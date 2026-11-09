
use clap::Parser;
use zedtest::CliArgs;
use colored::*;
use zedtest::filter::CompoundPredicate;
use zedtest::formatter::{CsvExporter, Formatter, OutputFormat};
use indicatif::{ProgressBar, ProgressStyle};
use zedtest::select::Selector;
use serde_json::Value;
use std::path::Path;
use tokio::fs::{self, File};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use zedtest::tui;

#[derive(Debug)]
pub struct StreamSummary {
    pub valid_count: usize,
    pub error_count: usize,
    pub skipped_blank_count: usize,
}

use tokio::sync::mpsc::{self};

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
        skipped_blank_count: 0,
    };

    // CSV mode buffers rows so the header can be the union of every record's
    // keys (with absent keys emitted as empty cells). Trade-off: rows are held
    // in memory until the stream ends, so CSV export is bounded by output size.
    // NDJSON / pretty-JSON stay fully streaming.
    let mut csv_exporter = if format == OutputFormat::Csv {
        Some(CsvExporter::new())
    } else {
        None
    };

    let pb = ProgressBar::new(total_bytes);
    pb.set_style(ProgressStyle::default_bar().template("{spinner:.green} Processing [{bar:40.magenta/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, ETA {eta})")?.progress_chars("█▓▒░"));

    //Consumer worker loop
    while let Some(line) = rx.recv().await {
        let bytes_processed = line.len() as u64 + 1;
        pb.inc(bytes_processed);
        let trimmed = line.trim();
        if trimmed.is_empty() {
            summary.skipped_blank_count += 1;
            continue;
        }

        // Parse each line exactly once. Lines that are valid JSON but lack the
        // required schema (string 'timestamp' and 'service') are routed to the
        // DLQ, matching the previous FlexibleLogEntry-based behavior.
        match serde_json::from_str::<Value>(trimmed) {
            Ok(json_val) => {
                let schema_ok = json_val.as_object().is_some()
                    && json_val.get("timestamp").and_then(Value::as_str).is_some()
                    && json_val.get("service").and_then(Value::as_str).is_some();
                if !schema_ok {
                    summary.error_count += 1;
                    let err_msg =
                        format!("Invalid log [missing field `timestamp` or `service`]: {trimmed}\n");
                    dlq_writer.write_all(err_msg.as_bytes()).await?;
                    continue;
                }

                //if where clause is provided, invoke predicate
                if let Some(ref pred) = predicate
                    && !pred.evaluate(&json_val)
                {
                    continue;
                }

                //if select clause is provided, invoke selector
                let output_json = match &selector {
                    Some(sel) => sel.project(&json_val),
                    None => json_val,
                };

                match &mut csv_exporter {
                    Some(exporter) => exporter.push(&output_json),
                    None => {
                        let formatted_str = Formatter::format(&output_json, format)?;
                        valid_writer.write_all(formatted_str.as_bytes()).await?;
                        valid_writer.write_all(b"\n").await?;
                    }
                }

                summary.valid_count += 1;
            }
            Err(err) => {
                //corrupted JSON
                summary.error_count += 1;
                let err_msg = format!("Corrupted log [{err}]: {trimmed}\n");
                dlq_writer.write_all(err_msg.as_bytes()).await?;
            }
        }
    }

    if let Some(exporter) = csv_exporter {
        let rendered = exporter.render()?;
        valid_writer.write_all(rendered.as_bytes()).await?;
        valid_writer.write_all(b"\n").await?;
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
            "  • Blank Lines Skipped: {}",
            summary.skipped_blank_count.to_string().yellow().bold()
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
