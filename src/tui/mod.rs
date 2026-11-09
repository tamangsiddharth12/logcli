pub mod app;
pub mod fetcher;
pub mod ui;

use crate::CliArgs;
use crate::filter::CompoundPredicate;
use app::{ActivePane, App};
use crossterm::{
    event::{
        self,
        Event::{self},
        KeyCode, KeyEvent, KeyEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use memmap2::Mmap;
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use tokio::sync::mpsc;

//Messages streamed from the background filter worker to the UI loop
pub enum FilterMsg {
    /// Indices of records matching the current predicate
    Found(Vec<usize>),
    /// Number of records scanned so far (no new matches in this batch)
    Progress(usize),
    /// Scan finished: total records scanned
    Done(usize),
}

//Handles incoming terminal events and routes key combinations based on active application state

pub fn handle_events(
    app: &mut App,
    fetcher: Option<&crate::tui::fetcher::RecordFetcher>,
    key: KeyEvent,
) -> io::Result<()> {
    if app.is_filtering {
        match key.code {
            KeyCode::Enter => {
                app.submit_filter();
                app.is_filtering = false;
            }
            KeyCode::Esc => {
                app.is_filtering = false;
            }
            KeyCode::Backspace => {
                app.filter_input.pop();
            }
            KeyCode::Char(c) => {
                app.filter_input.push(c);
            }
            _ => {}
        }
        return Ok(());
    }

    match key.code {
        KeyCode::Char('q') => {
            app.is_running = false;
        }
        KeyCode::Char('/') => {
            app.is_filtering = true;
        }
        KeyCode::Tab => {
            app.toggle_pane();
        }
        KeyCode::Esc => {
            app.active_pane = ActivePane::LogList;
        }
        KeyCode::Char('j') | KeyCode::Down => match app.active_pane {
            ActivePane::LogList => {
                app.next();
            }
            ActivePane::Inspector => {
                if let Some(f) = fetcher {
                    let (total_lines, viewport_height) = get_inspector_dimensions(app, f);
                    app.scroll_inspector_down(total_lines, viewport_height);
                }
            }
        },
        KeyCode::Char('k') | KeyCode::Up => match app.active_pane {
            ActivePane::LogList => {
                app.previous();
            }

            ActivePane::Inspector => {
                app.scroll_inspector_up();
            }
        },
        _ => {}
    }
    Ok(())
}

fn get_inspector_dimensions(app: &App, fetcher: &crate::tui::fetcher::RecordFetcher) -> (u16, u16) {
    let total_lines = match app.selected_log_index() {
        Some(idx) => match fetcher.fetch_record(idx) {
            Ok(val) => serde_json::to_string_pretty(&val)
                .unwrap_or_default()
                .lines()
                .count() as u16,
            Err(_) => 0,
        },
        None => 0,
    };
    let (_, term_height) = crossterm::terminal::size().unwrap_or((80, 24));
    let viewport_height = term_height.saturating_sub(8);
    (total_lines, viewport_height)
}
/// Scan the mmap'ed file line-by-line against `pred`. Match batches and
/// progress ticks are delivered to `emit`; returning `false` from `emit`
/// stops the scan early (e.g. the UI went away). Pure function so it can
/// be tested without a running UI loop.
pub fn scan_filter_indices(
    mmap: &Mmap,
    offsets: &[u64],
    pred: &CompoundPredicate,
    emit: &mut dyn FnMut(Vec<usize>, usize) -> bool,
    cancel: Option<&AtomicBool>,
) {
    let mut matches: Vec<usize> = Vec::new();
    for idx in 0..offsets.len() {
        if let Some(c) = cancel
            && c.load(Ordering::Relaxed)
        {
            return;
        }
        let start = offsets[idx] as usize;
        let end = offsets
            .get(idx + 1)
            .map(|&next| next as usize)
            .unwrap_or(mmap.len());
        let mut slice: &[u8] = &mmap[start..end];
        if slice.ends_with(b"\n") {
            slice = &slice[..slice.len() - 1];
        }
        if slice.ends_with(b"\r") {
            slice = &slice[..slice.len() - 1];
        }
        if let Ok(val) = serde_json::from_slice::<serde_json::Value>(slice)
            && pred.evaluate(&val)
        {
            matches.push(idx);
        }
        // Stream matches in batches so the UI updates live; also emit a
        // progress tick every 100k records so long scans show movement.
        if matches.len() >= 2048 && !emit(std::mem::take(&mut matches), idx + 1) {
            return;
        }
        if matches.len() < 2048 && (idx + 1) % 100_000 == 0 && !emit(Vec::new(), idx + 1) {
            return;
        }
    }
    emit(matches, offsets.len());
}

/// Run one filter scan on the blocking thread pool, streaming results over
/// the channel. The scan aborts implicitly if the receiver is dropped.
fn spawn_filter_job(
    path: String,
    offsets: Arc<Vec<u64>>,
    pred: CompoundPredicate,
    tx: mpsc::Sender<FilterMsg>,
) {
    tokio::task::spawn_blocking(move || {
        let send = |msg: FilterMsg| -> bool { tx.blocking_send(msg).is_ok() };

        let finished = std::fs::File::open(&path)
            .map_err(Box::new)
            .and_then(|file| unsafe { Mmap::map(&file) }.map_err(Box::new))
            .map(|mmap| {
                scan_filter_indices(&mmap, &offsets, &pred, &mut |matches, scanned| {
                    let msg = if matches.is_empty() {
                        FilterMsg::Progress(scanned)
                    } else {
                        FilterMsg::Found(matches)
                    };
                    send(msg)
                }, None)
            })
            .is_ok();

        if finished {
            let _ = send(FilterMsg::Done(offsets.len()));
        }
    });
}

pub async fn run_tui(args: CliArgs) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let file_path = args.file.clone();
    let tui_args = args;

    //1. Create a bounded channel holding the index
    let (tx, mut rx) = mpsc::channel::<Vec<u64>>(1);

    //2. Spawn the log processing engine in background task
    tokio::spawn(async move {
        if let Err(e) = crate::stream_log_file_mpsc_tui(tui_args, tx).await {
            eprintln!("Pipeline background error: {}", e);
        }
    });
    // Channel that streams background filter results into the event loop
    let (filter_tx, mut filter_rx) = mpsc::channel::<FilterMsg>(64);

    // 1. Setup Terminal into Raw Mode
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    let mut fetcher: Option<crate::tui::fetcher::RecordFetcher> = None;

    // 2. Main Event Loop
    while app.is_running {
        if let Ok(offsets) = rx.try_recv() {
            let offsets = Arc::new(offsets);
            app.record_offsets = offsets.clone();
            app.filtered_indices = (0..app.record_offsets.len()).collect();
            // Index is built and the file is fully available to the fetcher;
            // mark the app no longer "streaming".
            app.is_finished = true;

            // Initialize the fetcher once we have the offsets and the file
            let file = std::fs::File::open(&file_path)?;
            let mmap = unsafe { memmap2::Mmap::map(&file)? };
            fetcher = Some(crate::tui::fetcher::RecordFetcher::new(mmap, offsets));
        }

        // Drain background filter results without blocking the render loop
        while let Ok(msg) = filter_rx.try_recv() {
            match msg {
                FilterMsg::Found(batch) => app.filtered_indices.extend(batch),
                FilterMsg::Progress(scanned) => app.filter_scanned = scanned,
                FilterMsg::Done(scanned) => {
                    app.filter_scanned = scanned;
                    app.filter_in_progress = false;
                }
            }
        }

        // A filter was submitted and parsed: launch a background scan
        if let Some(pred) = app.pending_filter.take() {
            spawn_filter_job(
                file_path.clone(),
                app.record_offsets.clone(),
                pred,
                filter_tx.clone(),
            );
        }

        terminal.draw(|f| {
            if let Some(ref fetcher) = fetcher {
                ui::render(f, &app, fetcher)
            } else {
                ui::render_loading(f, &app)
            }
        })?;

        if event::poll(std::time::Duration::ZERO)? {
            if let Event::Key(key) = event::read()? {
                //only trigger on Key press
                if key.kind == KeyEventKind::Press
                    && let Err(e) = handle_events(&mut app, fetcher.as_ref(), key) {
                        eprintln!("Event Error Handling: {}", e);
                    }
            }
        } else {
            // Yield to Tokio so background tasks (indexer / filter worker)
            // can actually run.
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    }

    // 3. Restore Normal Terminal Screen
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}
