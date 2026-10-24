# Logcli CLI

A high‑throughput log ingestion and parsing engine written in Rust.

## Features

- **Fast streaming** using asynchronous I/O and memory‑mapped indexing.
- **Filtering** with a rich expression language (`WHERE` clause) supporting logical `AND` / `OR` and comparison operators.
- **Select** projection to output only desired fields.
- **Multiple output formats**: NDJSON, pretty‑JSON and CSV.
- **Terminal UI (TUI)** for interactive browsing of large log files.
- **Synthetic log generation** for testing and benchmarking.

## Installation

```bash
cargo install --locked --path .
```

Or download a pre‑built binary from the GitHub releases page.

## Usage

```bash
logcli -f /path/to/log.jsonl          # Process a log file (default NDJSON output)
logcli -f logs.jsonl -w "level==ERROR" -s "timestamp,service" -o csv
logcli -i -f large.log                 # Launch the interactive TUI
logcli -g 10000 -f synthetic.log       # Generate 10 k synthetic log lines
```

Run `logcli --help` for the full list of options.

## Development

- Run the test suite: `cargo test`
- Run the TUI: `cargo run -- -i -f sample.log`
- CI is configured via GitHub Actions (`.github/workflows/ci.yml`).

## License

MIT © Siddharth Tamang
