use super::app::{ActivePane, App};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Margin},
    style::{Color, Modifier, Style},
    widgets::{
        Block, Borders, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
        Wrap,
    },
};

pub fn render(f: &mut Frame, app: &App, fetcher: &crate::tui::fetcher::RecordFetcher) {
    let root_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), //Header and filter bar
            Constraint::Min(10),   //Main content split pane
            Constraint::Length(3), //Footer keybindings
        ])
        .split(f.area());

    let content_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(root_chunks[1]);

    // Render top: Search / Active filter Header
    let header_text = if app.is_filtering {
        format!(" FILTER PROMPT : /{}", app.filter_input)
    } else if app.filter_in_progress {
        format!(
            " ⏳ Filtering '{}' — scanned {} / {} records, {} matches so far...",
            app.filter_input,
            app.filter_scanned,
            app.record_offsets.len(),
            app.filtered_indices.len()
        )
    } else {
        let filter_status = match &app.active_predicate {
            Some(_) => format!("Active Filter --> {}", app.filter_input),
            None => "None (Showing All Logs)".to_string(),
        };
        format!(
            "Status: Normal Mode | Filter: {} | Press '/' to search, 'q' to quit",
            filter_status
        )
    };
    let header_block = Block::default()
        .borders(Borders::ALL)
        .border_style(if app.is_filtering {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        })
        .title(" Log Engine TUI");

    let header_paragraph = Paragraph::new(header_text).block(header_block);

    f.render_widget(header_paragraph, root_chunks[0]);

    // While typing a filter, show the terminal cursor right after the input
    // text so the prompt behaves like a normal text field.
    if app.is_filtering {
        let inner = root_chunks[0].inner(Margin {
            vertical: 1,
            horizontal: 1,
        });
        let prompt = format!(" FILTER PROMPT : /{}", app.filter_input);
        // chars().count() approximates display width (input is typed text)
        let cursor_x = inner.x + prompt.chars().count() as u16;
        f.set_cursor_position((
            cursor_x.min(inner.x.saturating_add(inner.width.saturating_sub(1))),
            inner.y,
        ));
    }

    //left pane -> Log stream list
    let total_visible = app.filtered_indices.len();
    let total_logs = app.record_offsets.len();
    let list_height = content_chunks[0].height.saturating_sub(2) as usize;
    let list_height = if list_height == 0 { 10 } else { list_height };

    let start = if app.selected_index >= list_height {
        app.selected_index.saturating_sub(list_height - 1)
    } else {
        0
    };
    let end = (start + list_height).min(total_visible);

    let window = if total_visible == 0 {
        &[]
    } else {
        &app.filtered_indices[start..end]
    };
    let items: Vec<ListItem> = window
        .iter()
        .enumerate()
        .map(|(idx, &log_idx)| {
            let actual_idx = start + idx;
            let line_str = match fetcher.fetch_record(log_idx) {
                Ok(val) => serde_json::to_string(&val).unwrap_or_default(),
                Err(_) => "Error loading record".to_string(),
                // Err(_) => {return None}
            };
            let style = if actual_idx == app.selected_index {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(line_str).style(style)
        })
        .collect();

    let is_loglist_active = app.active_pane == ActivePane::LogList;
    let list_border_style = if is_loglist_active {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Reset)
    };

    let list_title = if is_loglist_active {
        format!("Matches: {} / {} [Active] ", total_visible, total_logs)
    } else {
        format!("Matches {} / {} ", total_visible, total_logs)
    };

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(list_title)
            .border_style(list_border_style),
    );
    f.render_widget(list, content_chunks[0]);

    //Right pane: Pretty print JSON
    let json_detail = match app.selected_log_index() {
        Some(idx) => match fetcher.fetch_record(idx) {
            Ok(val) => serde_json::to_string_pretty(&val).unwrap_or_default(),
            Err(_) => "Error loading record".to_string(),
        },
        None => "No Record selected".to_string(),
    };

    let total_lines = json_detail.lines().count() as u16;
    let inspector_area = content_chunks[1];
    let viewport_height = inspector_area.height.saturating_sub(2);

    let is_inspector_active = app.active_pane == ActivePane::Inspector;
    let inspector_border_style = if is_inspector_active {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Reset)
    };

    let inspector_title = if is_inspector_active {
        "H Record Details [ACTIVE - Tab to switch] "
    } else {
        "H Record Details [Tab to focus] "
    };

    let inspector = Paragraph::new(json_detail)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(inspector_title)
                .border_style(inspector_border_style),
        )
        .scroll((app.inspector_scroll, 0))
        .wrap(Wrap { trim: false });

    f.render_widget(inspector, inspector_area);

    //Render visual scrollbar if JSON height exceeds inspector viewport
    // Inspector Scrollbar
    let max_scroll = total_lines.saturating_sub(viewport_height);
    if max_scroll > 0 {
        let mut scrollbar_state =
            ScrollbarState::new(max_scroll as usize).position(app.inspector_scroll as usize);

        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("⬆"))
            .end_symbol(Some("⬇"));

        f.render_stateful_widget(
            scrollbar,
            inspector_area.inner(Margin {
                vertical: 1,
                horizontal: 0,
            }),
            &mut scrollbar_state,
        );
    }

    //3. Footer Bar
    let status_status = if app.filter_in_progress {
        "STATUS: Filtering ⏳"
    } else if app.is_finished {
        "STATUS: Complete ⚪"
    } else {
        "STATUS: Streaming 🟢"
    };
    let keys_hint = if app.is_filtering {
        "Keys: type query | [Enter] apply | [Esc] cancel | [Ctrl+C] quit"
    } else {
        "Keys: [/] Filter | [↑/↓] Navigate | [q] Quit"
    };
    let status_text = format!(" {} | {}", status_status, keys_hint);
    let status_bar =
        Paragraph::new(status_text).style(Style::default().bg(Color::DarkGray).fg(Color::White));
    f.render_widget(status_bar, root_chunks[2]);
}

pub fn render_loading(f: &mut Frame, _app: &App) {
    let root_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(3),
        ])
        .split(f.area());

    let loading_text = " ⏳ Indexing JSON file... Please wait... ";
    let loading_paragraph = Paragraph::new(loading_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Owl Log Engine "),
        )
        .alignment(Alignment::Center);

    f.render_widget(loading_paragraph, root_chunks[1]);
}
