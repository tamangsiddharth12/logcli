pub mod app;
pub mod fetcher;
pub mod ui;

use crate::CliArgs;
use app::{ActivePane, App};
use crossterm::{
    event::{
        self,
        Event::{self},
        KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use tokio::sync::mpsc;

//Handles incoming terminal events and routes key combinations based on active application state

pub fn handle_events(
    app: &mut App,
    fetcher: Option<&crate::tui::fetcher::RecordFetcher>,
    key: KeyEvent,
) -> io::Result<()> {
    if app.is_filtering {
        match key.code {
            KeyCode::Enter => {
                app.apply_filter(fetcher);
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
pub async fn run_tui(args: CliArgs) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let file_path = args.file.clone();
    let tui_args = CliArgs {
        file: file_path.clone(),
        generate_count: args.generate_count,
        buffer: args.buffer,
        valid_out: args.valid_out.clone(),
        dlq_out: args.dlq_out.clone(),
        where_clause: args.where_clause.clone(),
        select_clause: args.select_clause.clone(),
        format: args.format,
        tui: args.tui,
    };

    //1. Create a bounded channel holding the index
    let (tx, mut rx) = mpsc::channel::<Vec<u64>>(1);

    //2. Spawn the log processing engine in background task
    tokio::spawn(async move {
        if let Err(e) = crate::stream_log_file_mpsc_tui(tui_args, tx).await {
            eprintln!("Pipeline background error: {}", e);
        }
    });
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
            app.record_offsets = offsets.clone();
            app.filtered_indices = (0..app.record_offsets.len()).collect();

            // Initialize the fetcher once we have the offsets and the file
            let file = std::fs::File::open(&file_path)?;
            let mmap = unsafe { memmap2::Mmap::map(&file)? };
            fetcher = Some(crate::tui::fetcher::RecordFetcher::new(mmap, offsets));
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
                if key.kind == KeyEventKind::Press {
                    // if let Ok(res) = handle_events(&mut app, fetcher.as_ref(), key)? {
                    //     if res {
                    //         return Ok(());
                    //     }
                    // }
                    if let Err(e) = handle_events(&mut app, fetcher.as_ref(), key) {
                        eprintln!("Event Error Handling: {}", e);
                    }
                }
            }
        } else {
            // 🚀 CRITICAL FIX: Yield to Tokio!
            // This allows the background file reader to actually run.
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    }

    // 3. Restore Normal Terminal Screen
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}
