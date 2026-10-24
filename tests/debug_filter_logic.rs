#[test]
fn test_debug_filter_logic() {
    use zedtest::filter::CompoundPredicate;
    use zedtest::tui::fetcher::RecordFetcher;
    use std::fs::File;
    use std::io::Write;
    use std::path::Path;
    use memmap2::Mmap;

    let tmp_path = Path::new(&std::env::temp_dir()).join("debug_filter.json");
    let mut file = File::create(&tmp_path).expect("cannot create temp file");
    let line1 = r#"{"level":"INFO","http":{"status":200}}"#;
    let line2 = r#"{"level":"ERROR","http":{"status":500}}"#;
    // write line1 + newline + line2 + newline
    file.write_all(line1.as_bytes()).unwrap();
    file.write_all(b"\n").unwrap();
    file.write_all(line2.as_bytes()).unwrap();
    file.write_all(b"\n").unwrap();
    file.flush().unwrap();
    // offsets: start of each line
    let offsets = vec![0u64, (line1.len() + 1) as u64];
    let mmap = unsafe { Mmap::map(&File::open(&tmp_path).unwrap()).unwrap() };
    let fetcher = RecordFetcher::new(mmap, offsets);
    let pred = CompoundPredicate::parse("level==INFO").unwrap();
    let val = fetcher.fetch_record(0).expect("fetch record 0");
    assert!(pred.evaluate(&val), "predicate should match first record");
}
