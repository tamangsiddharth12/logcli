use serde_json::Value;
use std::str::FromStr;

#[derive(Debug, PartialEq, Clone)]
pub enum Operator {
    Eq,    // ==
    NotEq, // !=
    Gt,    // >
    Gte,   //>=
    Lt,    // <
    Lte,   //<=
}

#[derive(Debug, Clone, PartialEq)]
pub enum LogicalOperator {
    And,
    Or,
}

#[derive(Debug, Clone)]
pub struct CompoundPredicate {
    pub predicates: Vec<Predicate>,
    pub op: LogicalOperator,
}

#[derive(Debug, Clone)]
pub struct Predicate {
    //JSON pointer path (e.g. "/http/status")
    pub pointer: String,
    pub op: Operator,

    //Target value to compare against
    pub expected: Value,
}

impl Predicate {
    //parses a filter string like "http.status==500" pr service != "auth-service"
    pub fn parse(expr: &str) -> Result<Predicate, String> {
        let expr = expr.trim();

        // 1. identify the operator and split key/value
        let (raw_key, op, raw_val) = if let Some((k, v)) = expr.split_once("==") {
            (k, Operator::Eq, v)
        } else if let Some((k, v)) = expr.split_once("!=") {
            (k, Operator::NotEq, v)
        } else if let Some((k, v)) = expr.split_once(">=") {
            (k, Operator::Gte, v)
        } else if let Some((k, v)) = expr.split_once("<=") {
            (k, Operator::Lte, v)
        } else if let Some((k, v)) = expr.split_once(">") {
            (k, Operator::Gt, v)
        } else if let Some((k, v)) = expr.split_once("<") {
            (k, Operator::Lt, v)
        } else {
            return Err(format!(
                "Invalid filter expression: {}. Missing operator (==, !=, >, <, >=, <=)",
                expr
            ));
        };

        // convert dot notation http.status to /http/status
        let clean_key = raw_key.trim();
        let pointer = if clean_key.starts_with('/') {
            clean_key.to_string()
        } else {
            let replaced_path: String = format!("/{}", clean_key.replace('.', "/"));
            replaced_path
            // format!("/{}", clean_key.replace('.', "/"))
        };

        //infer value type (number, bool, string)

        let clean_val = raw_val.trim().trim_matches('"').trim_matches('\'');
        let expected = if let Ok(i) = clean_val.parse::<i64>() {
            Value::Number(i.into())
        } else if let Ok(num) = clean_val.parse::<f64>() {
            serde_json::Number::from_f64(num)
                .map(Value::Number)
                .unwrap_or_else(|| Value::String(clean_val.to_string()))
        } else if let Ok(b) = clean_val.parse::<bool>() {
            Value::Bool(b)
        } else {
            Value::String(clean_val.to_string())
        };

        Ok(Self {
            pointer,
            op,
            expected,
        })
    }

    pub fn evaluate(&self, log: &Value) -> bool {
        //Navigate via JSON pointer, fail immediately if key is absent
        let actual = match log.pointer(&self.pointer) {
            Some(v) => v,
            None => return false,
        };

        match &self.op {
            Operator::Eq => actual == &self.expected,
            Operator::NotEq => actual != &self.expected,
            Operator::Gt => compare_values(actual, &self.expected, |a, b| a > b),
            Operator::Gte => compare_values(actual, &self.expected, |a, b| a >= b),
            Operator::Lt => compare_values(actual, &self.expected, |a, b| a < b),
            Operator::Lte => compare_values(actual, &self.expected, |a, b| a <= b),
        }
    }
}

fn compare_values<F>(actual: &Value, expected: &Value, cmp: F) -> bool
where
    F: Fn(f64, f64) -> bool,
{
    match (actual.as_f64(), expected.as_f64()) {
        (Some(a), Some(b)) => cmp(a, b),
        _ => false, //non numeric values cannot satisfy order comparisons
    }
}

impl CompoundPredicate {
    //Parse expresession with 'AND' , 'OR', or single predicates.
    // example
    // - http.status == 500 AND service == auth-service
    // - level == ERROR OR response_time_ms >= 1000
    // - http.status == 500
    pub fn parse(expr: &str) -> Result<Self, String> {
        let trimmed = expr.trim();
        if trimmed.contains(" AND ") {
            let mut predicates = Vec::new();
            for part in trimmed.split(" AND ") {
                predicates.push(Predicate::parse(part.trim())?);
            }
            Ok(Self {
                predicates,
                op: LogicalOperator::And,
            })
        } else if trimmed.contains(" OR ") {
            let mut predicates = Vec::new();
            for part in trimmed.split(" OR ") {
                predicates.push(Predicate::parse(part.trim())?);
            }
            Ok(Self {
                predicates,
                op: LogicalOperator::Or,
            })
        } else {
            let single = Predicate::parse(trimmed)?;
            Ok(Self {
                predicates: vec![single],
                op: LogicalOperator::And,
            })
        }
    }

    pub fn evaluate(&self, log: &Value) -> bool {
        // Goal of .all():
        // Verify that every single predicate in self.predicates returns true.
        // Closure: |p| p.evaluate(log) takes each SinglePredicate (p) and calls its .evaluate() method on the current log record (log).
        // Short-Circuit Mechanics:
        // .all() iterates through the list one predicate at a time.
        // If predicate #1 returns true, .all() continues to predicate #2.
        // The moment any predicate returns false, .all() stops immediately. It returns false to the caller without ever evaluating predicates #2, #3, or #4.
        //
        // Goal of .any():
        // Verify if at least one predicate in self.predicates returns true.
        // Short-Circuit Mechanics:
        // .any() iterates through the list one predicate at a time.
        // If predicate #1 returns false, .any() continues to predicate #2.
        // The moment any predicate returns true, .any() stops immediately. It returns true to the caller without checking the rest of the list.
        match self.op {
            //.all() short-circuiting on the first 'false'
            LogicalOperator::And => self.predicates.iter().all(|p| p.evaluate(log)),
            LogicalOperator::Or => self.predicates.iter().any(|p| p.evaluate(log)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FilterEngine {
    pub compound: CompoundPredicate,
}

impl FromStr for CompoundPredicate {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        CompoundPredicate::parse(s)
    }
}
impl FilterEngine {
    pub fn new(expr: &str) -> Result<Self, String> {
        let compound = CompoundPredicate::parse(expr)?;
        Ok(Self { compound })
    }

    pub fn matches(&self, log: &Value) -> bool {
        self.compound.evaluate(log)
    }

    pub fn filter_indices(&self, logs: &[Value]) -> Vec<usize> {
        logs.iter()
            .enumerate()
            .filter(|(_, log)| self.matches(log))
            .map(|(idx, _)| idx)
            .collect()
    }

    pub fn eval_compound(pred: &CompoundPredicate, log: &Value) -> bool {
        pred.evaluate(log)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_compound_and_success() {
        let log = json!({
            "service": "auth-service",
            "http": {"status": 500},
        });

        let pred =
            CompoundPredicate::parse("service==auth-service AND http.status == 500").unwrap();
        assert!(pred.evaluate(&log))
    }

    #[test]
    fn test_compound_and_failure() {
        let log = json!({
            "service": "auth-service",
            "http": {"status": 200},
        });

        let pred =
            CompoundPredicate::parse("service==auth-service AND http.status == 500").unwrap();
        assert!(!pred.evaluate(&log))
    }

    #[test]
    fn test_compound_or() {
        let log = json!({
            "level": "ERROR",
            "http": {"status": 200},
        });

        let pred = CompoundPredicate::parse("level==ERROR OR http.status == 200").unwrap();
        assert!(pred.evaluate(&log))
    }

    #[test]
    fn test_single_predicate() {
        let log = json!({
            "level": "ERROR",
            "http": {"status": 200},
        });

        let pred = CompoundPredicate::parse("level==ERROR").unwrap();
        assert!(pred.evaluate(&log))
    }

    #[test]
    fn test_nested_http_status_eq() {
        let pred = Predicate::parse("http.status == 500").unwrap();
        let log = json!({
            "timestamp": "2026-08-01T12:00:01Z",
            "service": "auth-service",
            "http": {
                "status": 500,
                "method": "POST"
            }
        });

        assert_eq!(pred.pointer, "/http/status");
        assert!(pred.evaluate(&log));
    }

    #[test]
    fn test_predicate_eq_string() {
        let pred = Predicate::parse("level==INFO").unwrap();
        assert_eq!(pred.pointer, "/level");
        assert_eq!(pred.op, crate::filter::Operator::Eq);
        assert_eq!(pred.expected, json!("INFO"));
        let log = json!({"level": "INFO"});
        assert!(pred.evaluate(&log));
    }

    #[test]
    fn test_predicate_gte_number() {
        let pred = Predicate::parse("http.status>=100").unwrap();
        assert_eq!(pred.pointer, "/http/status");
        assert_eq!(pred.op, crate::filter::Operator::Gte);
        let log = json!({"http": {"status": 200}});
        assert!(pred.evaluate(&log));
        let log2 = json!({"http": {"status": 50}});
        assert!(!pred.evaluate(&log2));
    }

    #[test]
    fn test_compound_and() {
        let comp = CompoundPredicate::parse("level==INFO AND http.status>=100").unwrap();
        let log = json!({"level": "INFO", "http": {"status": 200}});
        assert!(comp.evaluate(&log));
        let log2 = json!({"level": "INFO", "http": {"status": 50}});
        assert!(!comp.evaluate(&log2));
    }

    // fn test_compound_or() {
    //     let comp = CompoundPredicate::parse("level==ERROR OR http.status>=100").unwrap();
    //     let log = json!({"level": "INFO", "http": {"status": 200}});
    //     assert!(comp.evaluate(&log));
    //     let log2 = json!({"level": "ERROR", "http": {"status": 50}});
    //     assert!(comp.evaluate(&log2));
    //     let log3 = json!({"level": "INFO", "http": {"status": 50}});
    //     assert!(!comp.evaluate(&log3));
    // }
    #[test]
    fn test_predicate_invalid_operator() {
        let err = Predicate::parse("level!=INFO"); // valid operator
        assert!(err.is_ok()); // ensure parsing works for !=
        let err2 = Predicate::parse("level~INFO");
        assert!(err2.is_err());
    }
}
