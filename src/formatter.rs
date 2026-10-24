use clap::ValueEnum;
use serde_json::Value;

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
        write_header: bool,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        match format {
            OutputFormat::Ndjson => Ok(serde_json::to_string(val)?),
            OutputFormat::PrettyJson => Ok(serde_json::to_string_pretty(val)?),
            OutputFormat::Csv => {
                let mut wtr = csv::WriterBuilder::new()
                    .has_headers(write_header)
                    .from_writer(vec![]);

                if let Some(obj) = val.as_object() {
                    if write_header {
                        let headers: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
                        wtr.write_record(&headers);
                    }

                    let row: Vec<String> = obj
                        .values()
                        .map(|v| match v {
                            Value::String(s) => s.clone(),
                            Value::Null => String::new(),
                            _ => v.to_string(),
                        })
                        .collect();
                    wtr.write_record(&row)?;
                }

                let bytes = wtr.into_inner()?;
                Ok(String::from_utf8(bytes)?.trim_end().to_string())
            }
        }
    }
}
