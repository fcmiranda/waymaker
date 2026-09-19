use cba::bait::ResultExt;
use cba::unwrap;
use ratatui::text::{Line, Text};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use super::AppendOnly;

// Images?
#[derive(Debug, Clone)]
pub struct Preview {
    lines: AppendOnly<Line<'static>>,
    string: Arc<Mutex<Option<Text<'static>>>>,
    pub image: Arc<Mutex<Option<image::DynamicImage>>>,
    pub image_id: Arc<AtomicU64>,
    pub changed: Arc<AtomicBool>,
    /// Line offsets (0-based) in the rendered Text where each Mermaid diagram block starts.
    /// Updated on every Markdown re-render. Empty when not viewing a Markdown file.
    pub diagram_offsets: Arc<Mutex<Vec<usize>>>,
    /// Extracted Mermaid diagram source strings for the current Markdown file.
    pub diagram_sources: Arc<Mutex<Vec<String>>>,
    /// Index of the currently displayed diagram.
    pub current_diagram_idx: Arc<std::sync::atomic::AtomicUsize>,
}

impl Preview {
    pub fn has_string(&self) -> bool {
        unwrap!(self.string.lock().prefix("Previewer panicked")._elog())
            .as_ref()
            .is_some()
    }

    pub fn results(&self) -> Text<'_> {
        if let Some(s) = unwrap!(self.string.lock().prefix("Previewer panicked")._elog()).as_ref() {
            s.clone()
        } else {
            let output = self.lines.read();
            Text::from_iter(output.iter().map(|(_, line)| line.clone()))
        }
    }

    pub fn get_line(&self, index: usize) -> Option<Line<'static>> {
        if let Some(s) = unwrap!(self.string.lock().prefix("Previewer panicked")._elog()).as_ref() {
            s.lines.get(index).cloned()
        } else {
            let output = self.lines.read();
            output.get(index).cloned()
        }
    }

    pub fn results_window(&self, skip: usize, take: usize) -> Vec<Line<'static>> {
        if let Some(s) = unwrap!(self.string.lock().prefix("Previewer panicked")._elog()).as_ref() {
            s.lines.iter().skip(skip).take(take).cloned().collect()
        } else {
            let output = self.lines.read();
            output
                .iter()
                .skip(skip)
                .take(take)
                .map(|(_, line)| line.clone())
                .collect()
        }
    }

    pub fn lines_len(&self) -> usize {
        self.lines.len()
    }

    pub fn len(&self) -> usize {
        if let Some(s) = unwrap!(self.string.lock().prefix("Previewer panicked")._elog()).as_ref() {
            s.height()
        } else {
            self.lines.len()
        }
    }

    pub fn is_empty(&self) -> bool {
        if let Some(s) = unwrap!(self.string.lock().prefix("Previewer panicked")._elog()).as_ref() {
            s.height() == 0
        } else {
            let output = self.lines.read();
            output.iter().next().is_none()
        }
    }

    pub fn changed(&self) -> bool {
        self.changed.swap(false, Ordering::Relaxed)
    }

    pub fn new(
        lines: AppendOnly<Line<'static>>,
        string: Arc<Mutex<Option<Text<'static>>>>,
        image: Arc<Mutex<Option<image::DynamicImage>>>,
        image_id: Arc<AtomicU64>,
        changed: Arc<AtomicBool>,
        diagram_offsets: Arc<Mutex<Vec<usize>>>,
        diagram_sources: Arc<Mutex<Vec<String>>>,
        current_diagram_idx: Arc<std::sync::atomic::AtomicUsize>,
    ) -> Self {
        Self {
            lines,
            string,
            image,
            image_id,
            changed,
            diagram_offsets,
            diagram_sources,
            current_diagram_idx,
        }
    }
}
