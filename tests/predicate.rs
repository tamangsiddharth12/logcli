// use serde_json::json;
// use zedtest::filter::{CompoundPredicate, Predicate};
//
// #[test]
// fn test_predicate_eq_string() {
//     let pred = Predicate::parse("level==INFO").unwrap();
//     assert_eq!(pred.pointer, "/level");
//     assert_eq!(pred.op, zedtest::filter::Operator::Eq);
//     assert_eq!(pred.expected, json!("INFO"));
//     let log = json!({"level": "INFO"});
//     assert!(pred.evaluate(&log));
// }
//
// #[test]
// fn test_predicate_gte_number() {
//     let pred = Predicate::parse("http.status>=100").unwrap();
//     assert_eq!(pred.pointer, "/http/status");
//     assert_eq!(pred.op, zedtest::filter::Operator::Gte);
//     let log = json!({"http": {"status": 200}});
//     assert!(pred.evaluate(&log));
//     let log2 = json!({"http": {"status": 50}});
//     assert!(!pred.evaluate(&log2));
// }
//
// #[test]
// fn test_compound_and() {
//     let comp = CompoundPredicate::parse("level==INFO AND http.status>=100").unwrap();
//     let log = json!({"level": "INFO", "http": {"status": 200}});
//     assert!(comp.evaluate(&log));
//     let log2 = json!({"level": "INFO", "http": {"status": 50}});
//     assert!(!comp.evaluate(&log2));
// }
//
// #[test]
// fn test_compound_or() {
//     let comp = CompoundPredicate::parse("level==ERROR OR http.status>=100").unwrap();
//     let log = json!({"level": "INFO", "http": {"status": 200}});
//     assert!(comp.evaluate(&log));
//     let log2 = json!({"level": "ERROR", "http": {"status": 50}});
//     assert!(comp.evaluate(&log2));
//     let log3 = json!({"level": "INFO", "http": {"status": 50}});
//     assert!(!comp.evaluate(&log3));
// }
//
// #[test]
// fn test_predicate_invalid_operator() {
//     let err = Predicate::parse("level!=INFO"); // valid operator
//     assert!(err.is_ok()); // ensure parsing works for !=
//     let err2 = Predicate::parse("level~INFO");
//     assert!(err2.is_err());
// }
