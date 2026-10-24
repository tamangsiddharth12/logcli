use zedtest::tui::app::App;
use zedtest::tui::fetcher::RecordFetcher;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use memmap2::Mmap;

#[test]
fn test_apply_filter_without_fetcher() {
    let mut app = App::new();
    // Simulate three records (offsets are irrelevant when no fetcher)
    app.record_offsets = vec![0, 10, 20];
    app.filter_input = "level==INFO".to_string();
    app.apply_filter(None);
    assert!(app.active_predicate.is_some());
    assert_eq!(app.filtered_indices.len(), 3);
}

#[test]
fn test_apply_filter_with_fetcher() {
    // Create a temporary JSONL file with two lines
    let tmp_path = Path::new(&std::env::temp_dir()).join("test_app_filter_log.json");
    let mut file = File::create(&tmp_path).expect("cannot create temp file");
    let line1 = "{\"level\": \"INFO\", \"http\": {\"status\": 200}}\n";
    let line2 = "{\"level\": \"ERROR\", \"http\": {\"status\": 500}}\n";
    file.write_all(line1.as_bytes()).unwrap();
    file.write_all(line2.as_bytes()).unwrap();
    file.flush().unwrap();

    // Offsets: start of each line (0 and length of line1)
    let offsets = vec![0u64, line1.len() as u64];
    let mmap = unsafe { Mmap::map(&File::open(&tmp_path).unwrap()).unwrap() };
    let fetcher = RecordFetcher::new(mmap, offsets);

    let mut app = App::new();
    app.record_offsets = vec![0, line1.len() as u64];
    app.filter_input = "level==INFO".to_string();
    app.apply_filter(Some(&fetcher));
    // Only the first record should match the predicate
    assert_eq!(app.filtered_indices, vec![0]);
}
