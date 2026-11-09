use crate::filter::CompoundPredicate;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivePane {
    LogList,
    Inspector,
}

pub struct App {
    // Record start offsets, shared with the background filter worker.
    pub record_offsets: Arc<Vec<u64>>,
    pub selected_index: usize,
    pub filtered_indices: Vec<usize>,
    pub is_running: bool,
    pub filter_input: String,
    pub is_filtering: bool,
    pub active_predicate: Option<CompoundPredicate>,
    pub is_finished: bool,
    pub active_pane: ActivePane,
    pub inspector_scroll: u16,
    // Background filtering state. The heavy scan runs on a blocking worker;
    // `pending_filter` is consumed by the main loop to spawn it.
    pub pending_filter: Option<CompoundPredicate>,
    pub filter_in_progress: bool,
    pub filter_scanned: usize,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        Self {
            record_offsets: Arc::new(Vec::new()),
            selected_index: 0,
            filtered_indices: Vec::new(),
            is_running: true,
            filter_input: String::new(),
            is_filtering: false,
            active_predicate: None,
            is_finished: false,
            active_pane: ActivePane::LogList,
            inspector_scroll: 0,
            pending_filter: None,
            filter_in_progress: false,
            filter_scanned: 0,
        }
    }

    pub fn toggle_pane(&mut self) {
        self.active_pane = match self.active_pane {
            ActivePane::Inspector => ActivePane::LogList,
            ActivePane::LogList => ActivePane::Inspector,
        };
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

    /// Handle Enter pressed in the filter prompt. The actual record scan is
    /// NOT done here (it would freeze the UI for millions of records); a
    /// successful parse sets `pending_filter`, which the main loop spawns as
    /// a background worker that streams matching indices back.
    pub fn submit_filter(&mut self) {
        // Ignore submits while a background scan is already running.
        if self.filter_in_progress {
            return;
        }
        let trimmed = self.filter_input.trim();
        if trimmed.is_empty() {
            self.active_predicate = None;
            self.reset_filter_results();
            return;
        }
        if let Ok(pred) = trimmed.parse::<CompoundPredicate>() {
            self.active_predicate = Some(pred.clone());
            self.filtered_indices.clear();
            self.selected_index = 0;
            self.reset_inspector_scroll();
            self.filter_scanned = 0;
            self.filter_in_progress = true;
            self.pending_filter = Some(pred);
        }
        // Invalid expressions leave the previous filter in place.
    }

    /// Reset to an unfiltered view of all records.
    pub fn reset_filter_results(&mut self) {
        self.filtered_indices = (0..self.record_offsets.len()).collect();
        self.selected_index = 0;
        self.filter_in_progress = false;
        self.filter_scanned = 0;
    }
}
