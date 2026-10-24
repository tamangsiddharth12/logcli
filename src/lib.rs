use clap::Parser;
use colored::*;

use memchr::memchr;
use memmap2::Mmap;
use std::fs::File;
use tokio::sync::mpsc::Sender;

pub mod filter;
pub mod formatter;
pub mod select;
pub mod tui;
use formatter::OutputFormat;

#[derive(Debug, Parser)]
#[command(
    name = "logcli",
    author = "Siddharth",
    version = "1.0",
    about = "Log ingestion & parser engine"
)]
pub struct CliArgs {
    //path to the input file to process
    //#[arg(short, long, default_value = "production.log")]
    #[arg(short = 'f', long = "filename", required = true)]
    pub file: String,

    // Optional: Generate synthetic log dataset with specified line count
    #[arg(short = 'g', long = "generate")]
    pub generate_count: Option<usize>,
    //Channel buffer capacity
    //#[arg(short, long, default_value_t = 10)]
    #[arg(short = 'b', long = "buffersize", default_value_t = 10)]
    pub buffer: usize,

    //Destination path for valid parsed json logs
    //#[arg(long, default_value = "output/valid_logs.ndjson")]
    #[arg(short = 'v', default_value = "output/valid_logs.ndjson")]
    pub valid_out: String,

    //Destination path for corrupted error logs
    //#[arg(long, default_value = "output/dlq_errors.log")]
    #[arg(short = 'e', default_value = "output/dlq_errors.log")]
    pub dlq_out: String,

    //Filter expression using where clause (eg: --where "http.status == 500 or -w "service == auth-service")
    #[arg(short = 'w', long = "where")]
    pub where_clause: Option<String>,

    //Select expressing (e.g. -s "timestamp,service,http.status")
    #[arg(short = 's', long = "select")]
    pub select_clause: Option<String>,

    //Output format [ndjson, | pretty-json | csv ]
    #[arg(short = 'o', long = "format", default_value = "ndjson")]
    pub format: OutputFormat,

    // Launch interactive Terminal User Interface (TUI)
    #[arg(short = 'i', long = "tui")]
    pub tui: bool,
}

pub async fn stream_log_file_mpsc_tui(
    args: CliArgs,
    tx: Sender<Vec<u64>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let file = std::fs::File::open(&args.file)?;
    let mmap = unsafe { Mmap::map(&file)? };
    let data = &mmap[..];

    let mut offsets = Vec::new();
    offsets.push(0u64);

    let mut curr_pos = 0;
    while let Some(pos) = memchr(b'\n', &data[curr_pos..]) {
        curr_pos += pos + 1;
        offsets.push(curr_pos as u64);
    }

    // Send the entire index at once to the TUI
    if tx.send(offsets).await.is_err() {
        return Err("TUI channel closed".into());
    }

    Ok(())
}
