use serde_json::{Map, Value};

#[derive(Debug, Clone)]
pub struct Selector {
    //pairs of (original key name, rfc6901 pointer path)
    pub fields: Vec<(String, String)>,
}

impl Selector {
    pub fn parse(expr: &str) -> Self {
        let fields = expr
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|field| {
                let clean_field = field.trim_matches('"').trim_matches('\'');
                let path = clean_field.replace('.', "/");
                let pointer = if path.starts_with('/') {
                    path
                } else {
                    let replaced_path: String = format!("/{}", path);
                    replaced_path
                };
                (clean_field.to_string(), pointer)
            })
            .collect();
        Self { fields }
    }

    pub fn project(&self, log: &Value) -> Value {
        let mut projected_map = Map::new();
        for (field_name, pointer) in &self.fields {
            if let Some(val) = log.pointer(pointer) {
                projected_map.insert(field_name.clone(), val.clone());
            }
        }

        Value::Object(projected_map)
    }
}
