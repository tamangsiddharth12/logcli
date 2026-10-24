use serde_json::Value;

use crate::filter::{CompoundPredicate, FilterEngine};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivePane {
    LogList,
    Inspector,
}

pub struct App {
    pub record_offsets: Vec<u64>,
    pub selected_index: usize,
    pub filtered_indices: Vec<usize>,
    pub is_running: bool,
    pub filter_input: String,
    pub is_filtering: bool,
    pub active_predicate: Option<CompoundPredicate>,
    pub is_finished: bool,
    pub active_pane: ActivePane,
    pub inspector_scroll: u16,
}

impl App {
    pub fn new() -> Self {
        Self {
            record_offsets: Vec::new(),
            selected_index: 0,
            filtered_indices: Vec::new(),
            is_running: true,
            filter_input: String::new(),
            is_filtering: false,
            active_predicate: None,
            is_finished: false,
            active_pane: ActivePane::LogList,
            inspector_scroll: 0,
        }
    }

    pub fn toggle_pane(&mut self) {
        self.active_pane = match self.active_pane {
            ActivePane::Inspector => ActivePane::Inspector,
            ActivePane::LogList => ActivePane::LogList,
        }
    }

    pub fn scroll_inspector_down(&mut self, total_lines: u16, viewport_height: u16) {
        //prevent scrolling past the bottom of the content
        let max_scroll = total_lines.saturating_sub(viewport_height);
        if self.inspector_scroll < max_scroll {
            self.inspector_scroll = self.inspector_scroll.saturating_add(1);
        }
    }

    pub fn scroll_inspector_up(&mut self) {
        self.inspector_scroll = self.inspector_scroll.saturating_sub(1);
    }

    pub fn reset_inspector_scroll(&mut self) {
        self.inspector_scroll = 0;
    }

    pub fn next(&mut self) {
        let len = self.filtered_indices.len();
        if len > 0 && self.selected_index < len - 1 {
            self.selected_index += 1;
        }
        self.reset_inspector_scroll();
    }

    pub fn previous(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
        self.reset_inspector_scroll();
    }

    pub fn selected_log_index(&self) -> Option<usize> {
        self.filtered_indices.get(self.selected_index).copied()
    }

    pub fn apply_filter(&mut self, fetcher: Option<&crate::tui::fetcher::RecordFetcher>) {
        let trimmed = self.filter_input.trim();

        if trimmed.is_empty() {
            self.active_predicate = None;
            self.filtered_indices = (0..self.record_offsets.len()).collect();
            self.selected_index = 0;
            return;
        }
        // Note: Filtering logic will be moved to a background worker in Phase 4.
        // For now, we leave the predicate set so the UI knows a filter is active.
        if let Ok(pred) = self.filter_input.parse::<CompoundPredicate>() {
            self.active_predicate = Some(pred);
            self.selected_index = 0;
            if let Some(f) = fetcher {
                let active_pred = self.active_predicate.as_ref().unwrap();
                self.filtered_indices = (0..self.record_offsets.len())
                    .filter(|&idx| match f.fetch_record(idx) {
                        Ok(val) => active_pred.evaluate(&val),
                        Err(_) => false,
                    })
                    .collect();
            } else {
                self.filtered_indices = (0..self.record_offsets.len()).collect();
            }
        }
    }
}
