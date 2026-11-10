# logcli

A high-throughput NDJSON (JSON-lines) log ingestion, filtering and parsing
engine written in Rust, with an interactive terminal UI (TUI) for browsing
large log files.

- **Fast streaming pipeline** — async I/O with bounded back-pressure, each
  line parsed exactly once; ~90 MB/s (≈570k records/s) in release mode
- **Filtering** (`-w`) — expression language with comparison and boolean
  connectors over nested fields
- **Projection** (`-s`) — emit only the fields you want
- **Output formats** — NDJSON (default), pretty-JSON, CSV
- **DLQ routing** — corrupted or schema-violating records are written to a
  dead-letter file with the reason, so a bad line never stops the run
- **Interactive TUI** — memory-mapped random access with an LRU cache,
  background filtering with live progress, JSON inspector pane
- **Synthetic dataset generator** — for benchmarks and testing

## Requirements

- Rust 1.85 or newer (the crate uses edition 2024)
- Input must be **newline-delimited JSON** (one JSON object per line).
  Pretty-printed JSON documents (a single object spanning many lines) are
  not valid input — the TUI's mmap indexer will treat each line as a record
  and show `Error loading record` for all but standalone fragments.
- Every record is validated against a minimal schema: `timestamp` (string)
  and `service` (string) are **required**; all other fields are optional and
  of any shape. Records missing either required field are treated as invalid
  and routed to the DLQ.

## Installation

```bash
cargo install --locked --path .
```

This installs the `logcli` binary. Or build locally:

```bash
cargo build --release
# binary at target/release/logcli
```

## Usage

```bash
# Process a file, NDJSON output (default)
logcli -f /path/to/log.ndjson

# Filter: only ERROR records for a specific service (default connector case-insensitive)
logcli -f logs.ndjson -w "level==ERROR AND service==auth-service"

# Filter + projection + CSV output
logcli -f logs.ndjson -w "http.status == 500 or level == ERROR" \
    -s "timestamp,service,http.status" -o csv

# Pretty-printed JSON output
logcli -f logs.ndjson -o pretty-json

# Interactive TUI
logcli -i -f large.ndjson

# Generate 10k synthetic log lines (good for benchmarks/tests)
logcli -g 10000 -f synthetic.ndjson
```

Run `logcli --help` for the full option list.

### Options

| Flag | Meaning | Default |
|---|---|---|
| `-f, --filename <FILE>` | Input file (NDJSON). Required | — |
| `-g, --generate <COUNT>` | Generate a synthetic NDJSON dataset of `<COUNT>` lines at `-f`, then process it | — |
| `-b, --buffersize <N>` | Channel buffer capacity between producer and consumer | `10` |
| `-v <PATH>` | Output path for valid records | `output/valid_logs.ndjson` |
| `-e <PATH>` | Output path for rejected records (DLQ) | `output/dlq_errors.log` |
| `-w, --where <EXPR>` | Filter expression (see below) | — |
| `-s, --select <LIST>` | Comma-separated fields to project (see below) | — |
| `-o, --format <FMT>` | Output format: `ndjson`, `pretty-json` or `csv` | `ndjson` |
| `-i, --tui` | Launch the interactive TUI instead of the batch pipeline | — |

When `-v` is left at its default, the filename adapts to the format choice
(`valid_logs.ndjson` / `valid_logs.csv` / `valid_logs.json`). Output
directories are created automatically.

## Filter language (`-w`)

Compare any nested field (dot notation) against a value:

- Operators: `==`, `!=`, `>`, `>=`, `<`, `<=`
- Connectors: `AND`, `OR` (case-insensitive; use only **one** kind per
  expression — mixing them is rejected with an error, there is no
  operator precedence yet)
- Value types are inferred: number, boolean, or string (quotes optional)
- Field paths translate to JSON pointers: `http.status` targets
  `{"http": {"status": ...}}`
- Records missing the compared field do not match (except `!=`, which is a
  strict value inequality — a missing field never matches anything)

Examples:

```bash
-w "level==ERROR"
-w "http.status >= 500"
-w "level==ERROR or http.status>=500"
-w "service==auth-service AND http.status > 200"
```

## Projection (`-s`)

`-s` takes a comma-separated list of fields; each is copied into the output
record under its original (dotted) name. Absent fields are silently skipped.

```bash
logcli -f logs.ndjson -s "timestamp,service,http.status"
# {"http.status":500,"service":"auth-service","timestamp":"2026-08-01T12:00:01Z"}
```

## Output formats

- **ndjson** (default) — one compact JSON object per line, fully streaming
- **pretty-json** — one pretty-printed JSON object per record
- **csv** — the header is the **sorted union of keys across all emitted
  records**: rows are aligned to those columns and absent keys become empty
  cells, so heterogeneous records no longer shift columns. CSV rows are
  buffered in memory until the run ends, so bound the output with `-w`/`-s`
  for very large inputs; NDJSON and pretty-JSON stay fully streaming.

Every run also prints a metrics report: elapsed time, file size and line
count, exported vs. DLQ vs. skipped-blank counts, MB/s and records/s.

## DLQ (dead-letter queue)

Rejected records are written one-per-line to `-e` with the reason prefix:

```
Corrupted log [<serde parse error>]: <original line>
Invalid log [missing field `timestamp` or `service`]: <original line>
```

Blank lines are counted and reported (`Blank Lines Skipped`) but not routed
to the DLQ.

## Interactive TUI (`-i`)

```
┌───────────────────────────────────────────────┐
│ header: filter prompt / active filter state   │
├──────────────────────────┬────────────────────┤
│ matches list             │ JSON inspector     │
├──────────────────────────┴────────────────────┤
│ status bar: mode-aware key hints              │
└───────────────────────────────────────────────┘
```

On startup the file is memory-mapped and a line-offset index is built, so
opening even multi-GB files is fast; records are parsed on demand and cached
in an LRU (1000 entries).

Keys:

| Key | Action |
|---|---|
| `/` | Open the filter prompt |
| type + `Enter` | Apply the filter — scanning runs on a **background thread** and results populate the list live while the header shows progress; the UI stays responsive |
| `Esc` | Leave/cancel the filter prompt |
| `Tab` | Toggle focus between the list pane and the inspector pane |
| `j` / `↓`, `k` / `↑` | Navigate the focused pane |
| `q` | Quit (normal mode; in the prompt `q` is typed text — press `Esc` first) |
| `Ctrl+C` | Quit from anywhere |

The list pane titles show `Matches: N / total`. Corrupt lines appear as
`Error loading record` in the list — expected when the input contains
intentionally corrupted synthetic records.

### Synthetic generator (`-g`)

Emits N lines of which, by design:
- ~80% are valid records (alternating two schemas),
- ~10% are valid JSON missing `timestamp`/`service` (→ DLQ),
- ~10% are truncated JSON (→ DLQ).

So a 20% DLQ rate on generated data is expected. For a clean (0% corrupt)
dataset, or a configurable corruption rate, filter the output or open an
issue — the corruption rate is currently hard-coded.

### Benchmarks

On a release build (Apple Silicon), end-to-end on a 100k-line / 16 MB
synthetic file:

- ~93 MB/s, ~577k lines/sec (including parsing, filtering, and I/O)
- A 5.5M-line / ~884 MB dataset streams in ~10 s

## Development

```bash
cargo test      # 17 tests (filter engine, TUI state, mmap fetcher)
cargo clippy    # zero warnings
cargo fmt       # optional
```

CI runs the test suite via GitHub Actions (`.github/workflows/ci.yml`).

## Known limitations

- Filter expressions cannot mix `AND` and `OR` (rejected loudly) and have no
  parentheses/precedence.
- Input must be NDJSON; single pretty-printed JSON documents are not
  supported.
- `service` and `timestamp` are hard-required schema fields.
- CSV export buffers all rows in memory until the stream ends.
- The synthetic generator's corruption rate is fixed at 20%.

## License

MIT © Siddharth Tamang
