#[test]
fn test_fetcher_reads_first_line() {
    use std::fs::File;
    use std::io::Write;
    use std::path::Path;
    use memmap2::Mmap;
    use zedtest::tui::fetcher::RecordFetcher;
    use serde_json::json;
    // create temp file
    let tmp_path = Path::new(&std::env::temp_dir()).join("test_fetcher.json");
    let mut file = File::create(&tmp_path).expect("cannot create temp");
    let line1 = "{\"level\": \"INFO\", \"http\": {\"status\": 200}}\n";
    let line2 = "{\"level\": \"ERROR\", \"http\": {\"status\": 500}}\n";
    file.write_all(line1.as_bytes()).unwrap();
    file.write_all(line2.as_bytes()).unwrap();
    file.flush().unwrap();
    let offsets = vec![0u64, line1.len() as u64];
    let mmap = unsafe { Mmap::map(&File::open(&tmp_path).unwrap()).unwrap() };
    let fetcher = RecordFetcher::new(mmap, offsets);
    let val = fetcher.fetch_record(0).expect("fetch record 0");
    assert_eq!(val, json!({"level": "INFO", "http": {"status": 200}}));
}
