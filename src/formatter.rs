use clap::ValueEnum;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, ValueEnum)]
pub enum OutputFormat {
    Ndjson,
    PrettyJson,
    Csv,
}

pub struct Formatter;

impl Formatter {
    pub fn format(
        val: &Value,
        format: OutputFormat,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        match format {
            OutputFormat::Ndjson => Ok(serde_json::to_string(val)?),
            OutputFormat::PrettyJson => Ok(serde_json::to_string_pretty(val)?),
            // CSV is rendered by CsvExporter after the stream ends, so the
            // header can cover the union of every record's keys.
            OutputFormat::Csv => Err(
                "CSV output is collected and rendered by CsvExporter after the stream finishes"
                    .into(),
            ),
        }
    }
}

/// Buffered CSV exporter. Collects all projected records, then renders a
/// header from the sorted union of their keys and rows aligned to those
/// columns (missing keys become empty cells). This fixes the misalignment
/// that occurred when records had heterogeneous key sets.
///
/// Memory note: rows are held in memory until the stream ends, so CSV export
/// size is bounded by output size. NDJSON / pretty-JSON stay fully streaming.
pub struct CsvExporter {
    rows: Vec<BTreeMap<String, Value>>,
}

impl CsvExporter {
    pub fn new() -> Self {
        Self { rows: Vec::new() }
    }

    pub fn push(&mut self, val: &Value) {
        if let Some(obj) = val.as_object() {
            let row: BTreeMap<String, Value> = obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            self.rows.push(row);
        }
    }

    pub fn render(&self) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // Union of keys across all rows, sorted for a deterministic header.
        let mut header: Vec<String> = Vec::new();
        for row in &self.rows {
            for key in row.keys() {
                if !header.contains(key) {
                    header.push(key.clone());
                }
            }
        }
        header.sort();

        let mut wtr = csv::WriterBuilder::new().from_writer(vec![]);

        wtr.write_record(&header)?;

        for row in &self.rows {
            let record: Vec<String> = header
                .iter()
                .map(|k| match row.get(k) {
                    Some(Value::String(s)) => s.clone(),
                    Some(Value::Null) | None => String::new(),
                    Some(v) => v.to_string(),
                })
                .collect();
            wtr.write_record(&record)?;
        }

        let bytes = wtr.into_inner()?;
        Ok(String::from_utf8(bytes)?.trim_end().to_string())
    }
}
