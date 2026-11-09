use zedtest::filter::CompoundPredicate;
use zedtest::tui::app::App;
use zedtest::tui::scan_filter_indices;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use memmap2::Mmap;

#[test]
fn test_submit_filter_clears_and_sets_pending() {
    let mut app = App::new();
    // Simulate three records; no scan has run yet
    app.record_offsets = Arc::new(vec![0, 10, 20]);
    app.filter_input = "level==INFO".to_string();
    app.submit_filter();

    assert!(app.active_predicate.is_some());
    // Work is handed to the background worker, not applied inline
    assert!(app.pending_filter.is_some());
    assert!(app.filter_in_progress);
    assert!(app.filtered_indices.is_empty());
}

#[test]
fn test_submit_filter_empty_resets_to_all() {
    let mut app = App::new();
    app.record_offsets = Arc::new(vec![0, 10, 20]);
    app.filtered_indices = vec![2];
    app.active_predicate = CompoundPredicate::parse("level==INFO".to_string().as_str()).ok();
    app.filter_input = "   ".to_string();
    app.submit_filter();

    assert!(app.active_predicate.is_none());
    assert!(app.pending_filter.is_none());
    assert_eq!(app.filtered_indices, vec![0, 1, 2]);
}

#[test]
fn test_submit_filter_invalid_keeps_previous() {
    let mut app = App::new();
    app.record_offsets = Arc::new(vec![0]);
    app.filter_input = "level~BAD".to_string();
    app.submit_filter();
    assert!(app.active_predicate.is_none());
    assert!(app.pending_filter.is_none());
}

#[test]
fn test_scan_filter_indices_matches_expected_lines() {
    // Temporary NDJSON file with two lines
    let tmp_path = Path::new(&std::env::temp_dir()).join("test_scan_filter.json");
    let mut file = File::create(&tmp_path).expect("cannot create temp file");
    let line1 = "{\"level\": \"INFO\", \"http\": {\"status\": 200}}\n";
    let line2 = "{\"level\": \"ERROR\", \"http\": {\"status\": 500}}\n";
    file.write_all(line1.as_bytes()).unwrap();
    file.write_all(line2.as_bytes()).unwrap();
    file.flush().unwrap();

    let mmap = unsafe { Mmap::map(&File::open(&tmp_path).unwrap()).unwrap() };
    let offsets = vec![0u64, line1.len() as u64];
    let pred = CompoundPredicate::parse("level==INFO").unwrap();

    let mut collected: Vec<usize> = Vec::new();
    scan_filter_indices(&mmap, &offsets, &pred, &mut |matches, _scanned| {
        collected.extend(matches);
        true
    }, None);

    // Only the first record should match the predicate
    assert_eq!(collected, vec![0]);
}
