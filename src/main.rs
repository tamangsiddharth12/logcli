
use clap::Parser;
use zedtest::CliArgs;
use colored::*;
use zedtest::filter::CompoundPredicate;
use zedtest::formatter::{Formatter, OutputFormat};
use indicatif::{ProgressBar, ProgressStyle};
use zedtest::select::Selector;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use tokio::fs::{self, File};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::sync::mpsc::Sender;
use zedtest::tui;


#[derive(Debug, Deserialize)]
pub struct FlexibleLogEntry {
    pub timestamp: String,
    pub service: String,
    pub level: Option<String>,

    #[serde(flatten)]
    pub extra_fields: HashMap<String, Value>,
}

#[derive(Debug)]
pub struct StreamSummary {
    pub valid_count: usize,
    pub error_count: usize,
    pub matched_count: usize,
}

use memmap2::Mmap;
use memchr::memchr;
use tokio::sync::mpsc::{self};
// use serde_json::Value;

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

pub async fn stream_log_file_mpsc(
    file_path: &str,
    channel_capacity: usize,
    valid_out_path: &str,
    dlq_out_path: &str,
    predicate: Option<CompoundPredicate>,
    selector: Option<Selector>,
    format: OutputFormat,
) -> Result<StreamSummary, Box<dyn std::error::Error + Send + Sync>> {
    //1. ensure destination directories exist
    if let Some(parent) = Path::new(valid_out_path).parent()
        && !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).await?;
        }
    if let Some(parent) = Path::new(dlq_out_path).parent()
        && !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).await?;
        }

    let file_metadata = tokio::fs::metadata(file_path).await?;
    let total_bytes = file_metadata.len();

    // create output file wrapped in high performance buffer writers
    let valid_file = File::create(valid_out_path).await?;
    let dlq_file = File::create(dlq_out_path).await?;
    let mut valid_writer = BufWriter::new(valid_file);
    let mut dlq_writer = BufWriter::new(dlq_file);

    //Produce task
    let (tx, mut rx) = mpsc::channel::<String>(channel_capacity);
    let path = file_path.to_string();

    let producer_handle = tokio::spawn(async move {
        let file = File::open(&path).await?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();
        while let Some(line) = lines.next_line().await? {
            if tx.send(line).await.is_err() {
                break;
            }
        }
        Ok::<(), std::io::Error>(())
    });

    let mut summary = StreamSummary {
        valid_count: 0,
        error_count: 0,
        matched_count: 0,
    };

    let pb = ProgressBar::new(total_bytes);
    pb.set_style(ProgressStyle::default_bar().template("{spinner:.green} Processing [{bar:40.magenta/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, ETA {eta})")?.progress_chars("█▓▒░"));

    //Consumer worker loop
    let mut is_first_record = true;

    while let Some(line) = rx.recv().await {
        let bytes_processed = line.len() as u64 + 1;
        pb.inc(bytes_processed);
        let trimmed = line.trim();
        if trimmed.is_empty() {
            // dlq_writer.write_all(b"Empty lines encountered\n").await?;
            // summary.error_count += 1;
            continue;
        }

        match serde_json::from_str::<FlexibleLogEntry>(trimmed) {
            Ok(_) => {
                let json_val = match serde_json::from_str::<Value>(trimmed) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                //if where clause is provided, invoke predicate
                if let Some(ref pred) = predicate
                    && !pred.evaluate(&json_val) {
                        continue;
                    }
                    // if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
                    //     if !pred.evaluate(&json_val) {
                    //         continue; //goes back to top to read another log line
                    //     }
                    // else {
                    //     continue; //goes back to top to read another log line
                    // }
                    // }

                //if select clause is provided, invoke selector
                let output_json = match &selector {
                    Some(sel) => sel.project(&json_val),
                    None => json_val,
                };

                // Output formatting
                let formatted_str = Formatter::format(&output_json, format, is_first_record)?;
                is_first_record = false;
                // let output_str = serde_json::to_string(&output_json)?;

                summary.matched_count += 1;
                summary.valid_count += 1;
                // valid_writer.write_all(output_str.as_bytes()).await?;
                valid_writer.write_all(formatted_str.as_bytes()).await?;
                valid_writer.write_all(b"\n").await?;
            }
            Err(err) => {
                //corrupted JSON
                summary.error_count += 1;
                let err_msg = format!("Corrupted log [{}]: {}\n", err, trimmed);
                dlq_writer.write_all(err_msg.as_bytes()).await?;
                // dlq_writer.write_all(b"\n").await?;
            }
        }
    }

    producer_handle.await??;

    valid_writer.flush().await?;
    dlq_writer.flush().await?;
    pb.finish_and_clear();
    Ok(summary)
}

// generating synthetic logs for testing
pub async fn generate_synthetic_logs(
    file_path: &str,
    total_lines: usize,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if let Some(parent) = Path::new(file_path).parent()
        && !parent.as_os_str().is_empty() {
            tokio::fs::create_dir_all(parent).await?;
        }

    println!(
        "{} Generating {} synthetic log lines to '{}'...",
        "⚙".yellow().bold(),
        total_lines,
        file_path
    );

    let pb = ProgressBar::new(total_lines as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template(
                "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})",
            )?
            .progress_chars("#>-"),
    );

    let file = File::create(file_path).await?;
    //BufWriter wraps the new file to batch small disk writes into 8KB chunks
    let mut writer = BufWriter::new(file);

    let valid_sample_1 = r#"{"timestamp": "2026-08-01T12:00:00Z", "service": "payment-service", "level": "INFO", "http": {"method": "POST", "path": "/api/v2/charge", "status": 200, "response_time_ms": 120}}"#;
    let valid_sample_2 = r#"{"timestamp": "2026-08-01T12:00:01Z", "service": "auth-service", "level": "ERROR", "http": {"method": "POST", "path": "/api/v1/login", "status": 500, "response_time_ms": 340}, "region": "ap-south-1", "user_id": 9842}"#;
    let missing_field_sample = r#"{"timestamp": "CORRUPTED_LINE_WITHOUT_VALID_JSON"}"#;
    let broken_syntax_sample = r#"{"timestamp": "2026-08-01T12:00:02Z", "service": "#;

    for i in 1..=total_lines {
        let line = match i % 10 {
            0 => missing_field_sample, //10% of corrupted JSON lines
            5 => broken_syntax_sample, //10% of empty lines,
            _ => {
                if i % 2 == 0 {
                    valid_sample_1
                } else {
                    valid_sample_2
                }
            }
        };
        writer.write_all(line.as_bytes()).await?;
        writer.write_all(b"\n").await?;

        if i % 5000 == 0 {
            pb.inc(5000);
        }
    }
    writer.flush().await?;
    pb.finish_with_message("Dataset ready");
    println!(
        "{}\n",
        "✔ Synthetic dataset generated successfully!".green().bold()
    );
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut input_params = CliArgs::parse();

    if input_params.tui {
        tui::run_tui(input_params).await?;
    } else {
        if input_params.valid_out == "output/valid_logs.ndjson" {
            input_params.valid_out = match input_params.format {
                OutputFormat::Ndjson => "output/valid_logs.ndjson".to_string(),
                OutputFormat::Csv => "output/valid_logs.csv".to_string(),
                OutputFormat::PrettyJson => "output/valid_logs.json".to_string(),
            };
        }
        println!(
            "{}",
            "==================================================".cyan()
        );
        println!(
            "{}",
            "   🚀 RUST LOG PIPELINE: HIGH THROUGHPUT ENGINE   "
                .bold()
                .green()
        );
        println!(
            "{}\n",
            "==================================================".cyan()
        );
        //let file_path = "src/production.log";
        let file_path = &input_params.file;
        let channel_capacity = input_params.buffer;
        let valid_out_path = &input_params.valid_out;
        let dlq_out_path = &input_params.dlq_out;

        if let Some(count) = input_params.generate_count {
            generate_synthetic_logs(file_path, count).await?;
        }

        // start timer
        let start_time = std::time::Instant::now();

        let predicate = match &input_params.where_clause {
            Some(expr) => match CompoundPredicate::parse(expr) {
                Ok(p) => {
                    println!("{} Compound Filter Compiled: {:?}", "🔍".yellow(), p);
                    Some(p)
                }
                Err(err) => {
                    eprintln!("{} Filter parse error: {}", "❌".red().bold(), err);
                    std::process::exit(1)
                }
            },
            None => None,
        };

        let selector = input_params.select_clause.as_deref().map(Selector::parse);
        if let Some(ref sel) = selector {
            println!(
                " Projection active: extracting keys {:?}",
                sel.fields.iter().map(|(f, _)| f).collect::<Vec<_>>()
            );
        };

        println!("{} Output Format: {:?}", "📦".blue(), input_params.format);
        println!("{} Pipeline Streaming Starting...\n", "⚡".bold().yellow());

        let summary = stream_log_file_mpsc(
            file_path,
            channel_capacity,
            valid_out_path,
            dlq_out_path,
            predicate,
            selector,
            input_params.format,
        )
        .await?;

        //Stop timer
        let elapsed = start_time.elapsed();

        let file_bytes = std::fs::metadata(&input_params.file)?.len();
        let file_size_mb = file_bytes as f64 / (1024.0 * 1024.0);
        let total_lines = summary.valid_count + summary.error_count;
        let elapsed_secs = elapsed.as_secs_f64();

        let lines_per_sec = total_lines as f64 / elapsed_secs;
        let mb_per_sec = file_size_mb / elapsed_secs;

        // 6. Print Professional Execution Report
        println!(
            "\n{}",
            "📊 EXECUTION & PERFORMANCE METRICS".bold().underline()
        );
        println!(
            "  • Elapsed Time:      {:.3}s",
            elapsed_secs.to_string().bold()
        );
        println!(
            "  • File Processed:    {:.2} MB ({} total lines)",
            file_size_mb, total_lines
        );
        println!(
            "  • Matched & Exported: {}",
            summary.valid_count.to_string().green().bold()
        );
        println!(
            "  • Routed to DLQ:     {}",
            summary.error_count.to_string().red().bold()
        );
        println!(
            "  • Throughput Speed:  {}",
            format!("{:.2} MB/s", mb_per_sec).yellow().bold()
        );
        println!(
            "  • Record Rate:       {}",
            format!("{:.0} lines/sec", lines_per_sec).cyan().bold()
        );
        println!("\n{}", "✨ Processing Complete!".green().bold());
    }
    Ok(())
}

// #[cfg(test)]
// mod test_cases {
//     use super::*;

//     #[test]
//     fn test_parse_ndjson_batch_skips_corrupted_lines() {
//         let raw = concat!(
//             r#"{"timestamp": "2026-07-24T15:00:00Z", "service": "auth", "level": "ERROR", "http": {"method": "POST", "path": "/login", "status": 500, "response_time_ms": 100}}"#,
//             "\n",
//             r#"CORRUPTED_LINE_HERE"#,
//             "\n",
//             r#"{"timestamp": "2026-07-24T15:00:01Z", "service": "payment", "level": "INFO", "http": {"method": "GET", "path": "/pay", "status": 200, "response_time_ms": 200}}"#
//         );
//         let logs = parse_ndjson_batch(raw);

//         assert_eq!(
//             logs.len(),
//             2,
//             "Should parse exactly 2 valid logs and drop 1 corrupted line"
//         );
//         assert_eq!(logs[0].service, "auth");
//         assert_eq!(logs[1].service, "payment");
//     }

//     #[test]
//     fn test_calculate_average_latency_normal_and_empty() {
//         let raw = concat!(
//             r#"{"timestamp": "2026-07-24T15:00:00Z", "service": "auth", "level": "INFO", "http": {"method": "GET", "path": "/a", "status": 200, "response_time_ms": 100}}"#,
//             "\n",
//             r#"{"timestamp": "2026-07-24T15:00:01Z", "service": "auth", "level": "INFO", "http": {"method": "GET", "path": "/b", "status": 200, "response_time_ms": 300}}"#
//         );
//         let logs = parse_ndjson_batch(raw);
//         let avg = calculate_average_latency(&logs);
//         assert_eq!(avg, 200.0, "Average of 100ms and 300ms should be 200ms");

//         let empty_logs: Vec<ProductionLog> = Vec::new();
//         let empty_avg = calculate_average_latency(&empty_logs);
//         assert_eq!(
//             empty_avg, 0.0,
//             "Empty logs slice should safely return 0.0 without panicking"
//         );
//     }

//     #[test]
//     fn test_extract_high_seveiry_alerts_criteria() {
//         let raw = concat!(
//             // 1. Status >= 400 (matches)
//             r#"{"timestamp": "2026-07-24T15:00:00Z", "service": "s1", "level": "INFO", "http": {"method": "GET", "path": "/1", "status": 404, "response_time_ms": 50}}"#,
//             "\n",
//             // 2. Level ERROR (matches)
//             r#"{"timestamp": "2026-07-24T15:00:01Z", "service": "s2", "level": "ERROR", "http": {"method": "GET", "path": "/2", "status": 200, "response_time_ms": 50}}"#,
//             "\n",
//             // 3. Level FATAL (matches)
//             r#"{"timestamp": "2026-07-24T15:00:02Z", "service": "s3", "level": "FATAL", "http": {"method": "GET", "path": "/3", "status": 200, "response_time_ms": 50}}"#,
//             "\n",
//             // 4. INFO + Status 200 (should BE FILTERED OUT)
//             r#"{"timestamp": "2026-07-24T15:00:03Z", "service": "s4", "level": "INFO", "http": {"method": "GET", "path": "/4", "status": 200, "response_time_ms": 50}}"#
//         );

//         let logs = parse_ndjson_batch(raw);
//         let alerts = extract_high_severity_alerts(logs);

//         assert_eq!(alerts.len(), 3, "Only 3 alerting records should pass");
//         assert_eq!(alerts[0].service, "s1");
//         assert_eq!(alerts[1].service, "s2");
//         assert_eq!(alerts[2].service, "s3");
//     }
// }
