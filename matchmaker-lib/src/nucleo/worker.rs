// Original code from https://github.com/helix-editor/helix (MPL 2.0)
// Modified by Squirreljetpack, 2025

use super::{Line, Span, Style, Text};
use bitflags::bitflags;
use std::{
    borrow::Cow,
    mem::take,
    sync::{
        Arc,
        atomic::{self, AtomicBool, AtomicU32, Ordering},
    },
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::{injector::WorkerInjector, query::PickerQuery};
use crate::{
    SSS,
    config::AutoscrollSettings,
    nucleo::Render,
    utils::text::{truncation_indicator, wrap_text, wrapping_indicator},
};

fn get_item_tier_and_clean_path<'a>(raw_str: &'a str, dir_first: bool) -> (u8, &'a str) {
    if !dir_first {
        return (2, raw_str);
    }

    let trimmed = raw_str.strip_prefix("./").unwrap_or(raw_str);
    if trimmed.starts_with('/') || trimmed.starts_with('\\') {
        let clean = if trimmed == "/" || trimmed == "\\" {
            trimmed
        } else {
            trimmed.trim_end_matches(|c| c == '/' || c == '\\')
        };
        return (2, clean);
    }

    let clean = trimmed.trim_end_matches(|c| c == '/' || c == '\\');
    let slash_count = clean.bytes().filter(|&b| b == b'/' || b == b'\\').count();

    if slash_count == 0 && !clean.is_empty() {
        let is_dir = raw_str.ends_with('/')
            || raw_str.ends_with('\\')
            || std::path::Path::new(clean).is_dir();
        if is_dir {
            (0, clean)
        } else {
            (1, clean)
        }
    } else {
        (2, clean)
    }
}

#[inline]
fn cmp_ascii_case_insensitive(a: &str, b: &str) -> std::cmp::Ordering {
    let mut a_bytes = a.bytes().map(|b| b.to_ascii_lowercase());
    let mut b_bytes = b.bytes().map(|b| b.to_ascii_lowercase());
    loop {
        match (a_bytes.next(), b_bytes.next()) {
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(&y);
                }
            }
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (None, None) => return std::cmp::Ordering::Equal,
        }
    }
}

#[inline]
fn count_slashes(s: &str) -> u64 {
    let mut count = 0u64;
    for &b in s.as_bytes() {
        if b == b'/' || b == b'\\' {
            count += 1;
        }
    }
    count
}

fn compute_item_score(
    total: u32,
    idx: usize,
    raw_path: &str,
    is_query_empty: bool,
    query_len: usize,
    snapshot_ref: Option<&crate::frecency::FrecencySnapshot>,
    frec_weight: u32,
    location_bias: u32,
    penalty: u32,
) -> u64 {
    let base_score = total.saturating_sub(idx as u32) as u64;
    let frecency_bonus = if !is_query_empty {
        snapshot_ref
            .map(|snap| snap.get_bonus_with_bias(raw_path, location_bias) * frec_weight)
            .unwrap_or(0) as u64
    } else {
        0
    };
    let effective_penalty = if is_query_empty || penalty == 0 {
        0
    } else if query_len <= 2 {
        penalty.saturating_mul(2)
    } else {
        penalty
    };
    let depth = if effective_penalty > 0 {
        count_slashes(raw_path)
    } else {
        0
    };

    base_score
        .saturating_add(frecency_bonus)
        .saturating_sub(depth * effective_penalty as u64)
}

type ColumnFormatFn<T> = Box<dyn for<'a> Fn(&'a T) -> Text<'a> + Send + Sync>;
type ColumnRawFn<T> = Box<dyn for<'a> Fn(&'a T) -> Cow<'a, str> + Send + Sync>;
pub struct Column<T> {
    pub name: Arc<str>,
    pub(super) format: ColumnFormatFn<T>,
    pub(super) raw: Option<ColumnRawFn<T>>,
    /// Whether the column should be passed to nucleo for matching and filtering.
    pub(super) filter: bool,
}

impl<T> Column<T> {
    pub fn new_boxed(name: impl Into<Arc<str>>, format: ColumnFormatFn<T>) -> Self {
        Self {
            name: name.into(),
            format,
            filter: true,
            raw: None,
        }
    }

    pub fn new<F>(name: impl Into<Arc<str>>, f: F) -> Self
    where
        F: for<'a> Fn(&'a T) -> Text<'a> + SSS,
    {
        Self {
            name: name.into(),
            format: Box::new(f),
            filter: true,
            raw: None,
        }
    }

    pub fn with_raw<F>(mut self, f: F) -> Self
    where
        F: for<'a> Fn(&'a T) -> Cow<'a, str> + SSS,
    {
        self.raw = Some(Box::new(f));
        self
    }

    /// Disable filtering.
    pub fn without_filtering(mut self) -> Self {
        self.filter = false;
        self
    }

    pub fn format<'a>(&self, item: &'a T) -> Text<'a> {
        (self.format)(item)
    }

    // Note: the characters should match the output of [`Self::format`]
    pub fn raw<'a>(&self, item: &'a T) -> Cow<'a, str> {
        if let Some(r) = &self.raw {
            (r)(item)
        } else {
            Cow::Owned((self.format)(item).to_string())
        }
    }
}

/// Worker: can instantiate, push, and get results. A view into computation.
///
/// Additionally, the worker can affect the computation via find and restart.
pub struct Worker<T>
where
    T: SSS,
{
    /// The inner `Nucleo` fuzzy matcher.
    pub nucleo: nucleo::Nucleo<T>,
    /// The last pattern that was matched against.
    pub query: PickerQuery,
    /// A pre-allocated buffer used to collect match indices when fetching the results
    /// from the matcher. This avoids having to re-allocate on each pass.
    pub col_indices_buffer: Vec<u32>,
    pub columns: Arc<[Column<T>]>,
    pub sort_threshold: crate::config::SortThreshold,
    pub depth_penalty: u32,
    pub frecency: bool,
    pub frecency_weight: u32,
    pub location_bias: u32,
    pub frecency_half_life_days: u32,
    pub sort_cap: usize,
    pub frecency_snapshot: Option<crate::frecency::FrecencySnapshot>,
    pub typo_tolerance: bool,
    pub dir_first: bool,
    pub sort_order: Option<crate::action::SortOrder>,
    pub mode_index: usize,
    pub matcher_dirty: Arc<AtomicBool>,
    notify_callback: Arc<arc_swap::ArcSwapOption<NotifyFn>>,

    // Background tasks which push to the injector check their version matches this or exit
    pub(super) version: Arc<AtomicU32>,
    // pub settings: WorkerSettings,
    pub group_header: Option<Box<dyn for<'a> Fn(&'a T) -> Option<Arc<str>> + Send + Sync>>,
    column_options: Vec<ColumnOptions>,
}

struct NotifyFn(Box<dyn Fn() + Send + Sync>);

// #[derive(Debug, Default)]
// pub struct WorkerSettings {
//     pub stable: bool,
// }

bitflags! {
    #[derive(Default, Clone, Debug)]
    pub struct ColumnOptions: u8 {
        const Optional = 1 << 0;
        const OrUseDefault = 1 << 2;
    }
}

impl<T: SSS> Worker<T> {
    /// Column names must be distinct!
    pub fn new(columns: impl IntoIterator<Item = Column<T>>, default_column: usize) -> Self {
        let columns: Arc<[_]> = columns.into_iter().collect();
        let matcher_columns = columns.iter().filter(|col| col.filter).count() as u32;

        let matcher_dirty = Arc::new(AtomicBool::new(false));
        let notify_callback: Arc<arc_swap::ArcSwapOption<NotifyFn>> =
            Arc::new(arc_swap::ArcSwapOption::empty());

        let dirty_ref = matcher_dirty.clone();
        let cb_ref = notify_callback.clone();

        let inner = nucleo::Nucleo::new(
            nucleo::Config::DEFAULT,
            Arc::new(move || {
                dirty_ref.store(true, Ordering::Release);
                if let Some(cb) = cb_ref.load().as_ref() {
                    (cb.0)();
                }
            }),
            None,
            matcher_columns,
        );

        Self {
            nucleo: inner,
            matcher_dirty,
            notify_callback,
            col_indices_buffer: Vec::with_capacity(128),
            query: PickerQuery::new(columns.iter().map(|col| &col.name).cloned(), default_column),
            column_options: vec![ColumnOptions::default(); columns.len()],
            group_header: None,
            columns,
            sort_threshold: crate::config::SortThreshold::NEVER,
            depth_penalty: 0,
            frecency: false,
            frecency_weight: 1,
            location_bias: 30,
            frecency_half_life_days: 7,
            sort_cap: 1000,
            frecency_snapshot: None,
            typo_tolerance: false,
            dir_first: false,
            sort_order: None,
            mode_index: 0,
            version: Arc::new(AtomicU32::new(0)),
        }
    }

    pub fn set_mode_index(&mut self, mode_index: usize) {
        self.mode_index = mode_index;
    }

    pub fn set_sort_order(&mut self, order: Option<crate::action::SortOrder>) {
        self.sort_order = order;
    }

    pub fn get_sort_order(&self) -> Option<crate::action::SortOrder> {
        self.sort_order
    }

    pub fn set_notify<F>(&self, f: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.notify_callback
            .store(Some(Arc::new(NotifyFn(Box::new(f)))));
    }

    pub fn set_column_options(&mut self, index: usize, options: ColumnOptions) {
        if options.contains(ColumnOptions::Optional) {
            self.nucleo
                .pattern
                .configure_column(index, nucleo::pattern::Variant::Optional)
        }

        self.column_options[index] = options
    }

    pub fn reverse_items(&mut self, reverse_items: bool) {
        self.nucleo.reverse_items(reverse_items);
    }

    pub fn injector(&self) -> WorkerInjector<T> {
        WorkerInjector {
            inner: self.nucleo.injector(),
            columns: self.columns.clone(),
            version: self.version.load(atomic::Ordering::Relaxed),
            picker_version: self.version.clone(),
        }
    }

    pub fn find(&mut self, line: &str) {
        if self.sort_threshold.is_smart() {
            let effective = self.sort_threshold.get_effective_threshold(line);
            self.nucleo.set_stability(effective);
        }
        let old_query = self.query.parse(line);
        if self.query == old_query {
            return;
        }
        for (i, column) in self
            .columns
            .iter()
            .filter(|column| column.filter)
            .enumerate()
        {
            let pattern = self
                .query
                .get(&column.name)
                .map(|s| &**s)
                .unwrap_or_else(|| {
                    self.column_options[i]
                        .contains(ColumnOptions::OrUseDefault)
                        .then(|| self.query.primary_column_query())
                        .flatten()
                        .unwrap_or_default()
                });

            let old_pattern = old_query
                .get(&column.name)
                .map(|s| &**s)
                .unwrap_or_else(|| {
                    self.column_options[i]
                        .contains(ColumnOptions::OrUseDefault)
                        .then(|| {
                            let name = self.query.primary_column_name()?;
                            old_query.get(name).map(|s| &**s)
                        })
                        .flatten()
                        .unwrap_or_default()
                });

            // Fastlane: most columns will remain unchanged after each edit.
            if pattern == old_pattern {
                continue;
            }
            let is_append = pattern.starts_with(old_pattern);

            self.nucleo.pattern.reparse(
                i,
                pattern,
                nucleo::pattern::CaseMatching::Smart,
                nucleo::pattern::Normalization::Smart,
                is_append,
            );
        }
    }

    pub fn get_nth(&self, n: u32) -> Option<&T> {
        let snapshot = self.nucleo.snapshot();
        let total = snapshot.matched_item_count();
        if n >= total {
            return None;
        }

        let query_str = self.query.primary_column_query().unwrap_or_default();
        let is_query_empty = query_str.is_empty();
        let query_len = query_str.len();

        let effective_dir_first = self.dir_first && self.mode_index == 0;
        let effective_sort_order = if self.mode_index == 0 {
            self.sort_order
        } else {
            None
        };
        let should_sort = effective_sort_order.is_some()
            || (!is_query_empty
                && ((self.frecency && self.frecency_snapshot.is_some())
                    || (self.depth_penalty > 0 && self.mode_index == 0)))
            || effective_dir_first;

        if should_sort {
            let total_sort = if effective_sort_order.is_some() {
                total
            } else if self.sort_cap > 0 {
                total.min(self.sort_cap as u32)
            } else {
                total
            };
            let mut items: Vec<_> = snapshot.matched_items(0..total_sort).enumerate().collect();
            let penalty = if is_query_empty || self.mode_index != 0 {
                0
            } else {
                self.depth_penalty
            };
            let frec_weight = self.frecency_weight;
            let snapshot_ref = if self.frecency {
                self.frecency_snapshot.as_ref()
            } else {
                None
            };
            let col0 = &self.columns[0];
            let scan_end = total.min(total_sort + 5000);
            if scan_end > total_sort {
                for (idx, item) in snapshot.matched_items(total_sort..scan_end).enumerate() {
                    let raw_path = col0.raw(item.data);
                    let has_frecency =
                        snapshot_ref.map_or(false, |snap| snap.has_bonus_fast(raw_path.as_ref()));
                    let is_direct = if effective_dir_first {
                        let (tier, _) = get_item_tier_and_clean_path(raw_path.as_ref(), true);
                        tier < 2
                    } else {
                        false
                    };
                    if has_frecency || is_direct {
                        items.push((total_sort as usize + idx, item));
                    }
                }
            }
            struct DecoratedItem<'a, T> {
                item: nucleo::Item<'a, T>,
                tier: u8,
                raw_path: Cow<'a, str>,
                clean_range: (usize, usize),
                score: u64,
                mtime: Option<std::time::SystemTime>,
                btime: Option<std::time::SystemTime>,
                size: Option<u64>,
                ext_range: Option<(usize, usize)>,
            }

            impl<'a, T> DecoratedItem<'a, T> {
                #[inline]
                fn clean(&self) -> &str {
                    &self.raw_path[self.clean_range.0..self.clean_range.1]
                }

                #[inline]
                fn ext(&self) -> &str {
                    if let Some((start, end)) = self.ext_range {
                        &self.raw_path[start..end]
                    } else {
                        ""
                    }
                }
            }

            let mut decorated: Vec<DecoratedItem<'_, T>> = items
                .into_iter()
                .map(|(idx, item)| {
                    let raw_path = col0.raw(item.data);
                    let score = compute_item_score(
                        total,
                        idx,
                        raw_path.as_ref(),
                        is_query_empty,
                        query_len,
                        snapshot_ref,
                        frec_weight,
                        self.location_bias,
                        penalty,
                    );
                    let (tier, clean) =
                        get_item_tier_and_clean_path(raw_path.as_ref(), effective_dir_first);
                    let clean_start = clean.as_ptr() as usize - raw_path.as_ref().as_ptr() as usize;
                    let clean_range = (clean_start, clean_start + clean.len());
                    let (mtime, btime, size, ext_range) = match effective_sort_order {
                        Some(
                            crate::action::SortOrder::Modified
                            | crate::action::SortOrder::ModifiedReverse,
                        ) => {
                            let m = std::fs::metadata(clean)
                                .or_else(|_| std::fs::symlink_metadata(clean))
                                .and_then(|meta| meta.modified())
                                .ok();
                            (m, None, None, None)
                        }
                        Some(
                            crate::action::SortOrder::Created
                            | crate::action::SortOrder::CreatedReverse,
                        ) => {
                            let meta = std::fs::metadata(clean)
                                .or_else(|_| std::fs::symlink_metadata(clean))
                                .ok();
                            let b = meta
                                .as_ref()
                                .and_then(|m| m.created().ok())
                                .or_else(|| meta.as_ref().and_then(|m| m.modified().ok()));
                            (None, b, None, None)
                        }
                        Some(
                            crate::action::SortOrder::Size | crate::action::SortOrder::SizeReverse,
                        ) => {
                            let s = std::fs::metadata(clean)
                                .or_else(|_| std::fs::symlink_metadata(clean))
                                .map(|meta| meta.len())
                                .ok();
                            (None, None, s, None)
                        }
                        Some(
                            crate::action::SortOrder::Extension
                            | crate::action::SortOrder::ExtensionReverse,
                        ) => {
                            let ext = std::path::Path::new(clean)
                                .extension()
                                .and_then(|e| e.to_str());
                            let range = ext.map(|e| {
                                let start =
                                    e.as_ptr() as usize - raw_path.as_ref().as_ptr() as usize;
                                (start, start + e.len())
                            });
                            (None, None, None, range)
                        }
                        _ => (None, None, None, None),
                    };
                    DecoratedItem {
                        item,
                        tier,
                        raw_path,
                        clean_range,
                        score,
                        mtime,
                        btime,
                        size,
                        ext_range,
                    }
                })
                .collect();

            decorated.sort_unstable_by(|a, b| {
                if let Some(sort_order) = effective_sort_order {
                    use crate::action::SortOrder;
                    if effective_dir_first && a.tier != b.tier {
                        return a.tier.cmp(&b.tier);
                    }

                    let ord = match sort_order {
                        SortOrder::Alphabetical => cmp_ascii_case_insensitive(a.clean(), b.clean())
                            .then_with(|| a.clean().cmp(b.clean())),
                        SortOrder::AlphabeticalReverse => {
                            cmp_ascii_case_insensitive(b.clean(), a.clean())
                                .then_with(|| b.clean().cmp(a.clean()))
                        }
                        SortOrder::Natural => {
                            crate::utils::string::natural_cmp(a.clean(), b.clean())
                                .then_with(|| a.clean().cmp(b.clean()))
                        }
                        SortOrder::NaturalReverse => {
                            crate::utils::string::natural_cmp(b.clean(), a.clean())
                                .then_with(|| b.clean().cmp(a.clean()))
                        }
                        SortOrder::Modified => {
                            let a_time = a.mtime.unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                            let b_time = b.mtime.unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                            a_time.cmp(&b_time).then_with(|| {
                                crate::utils::string::natural_cmp(a.clean(), b.clean())
                            })
                        }
                        SortOrder::ModifiedReverse => {
                            let a_time = a.mtime.unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                            let b_time = b.mtime.unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                            b_time.cmp(&a_time).then_with(|| {
                                crate::utils::string::natural_cmp(a.clean(), b.clean())
                            })
                        }
                        SortOrder::Created => {
                            let a_time = a.btime.unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                            let b_time = b.btime.unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                            a_time.cmp(&b_time).then_with(|| {
                                crate::utils::string::natural_cmp(a.clean(), b.clean())
                            })
                        }
                        SortOrder::CreatedReverse => {
                            let a_time = a.btime.unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                            let b_time = b.btime.unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                            b_time.cmp(&a_time).then_with(|| {
                                crate::utils::string::natural_cmp(a.clean(), b.clean())
                            })
                        }
                        SortOrder::Size => {
                            let a_size = a.size.unwrap_or(0);
                            let b_size = b.size.unwrap_or(0);
                            a_size.cmp(&b_size).then_with(|| {
                                crate::utils::string::natural_cmp(a.clean(), b.clean())
                            })
                        }
                        SortOrder::SizeReverse => {
                            let a_size = a.size.unwrap_or(0);
                            let b_size = b.size.unwrap_or(0);
                            b_size.cmp(&a_size).then_with(|| {
                                crate::utils::string::natural_cmp(a.clean(), b.clean())
                            })
                        }
                        SortOrder::Extension => cmp_ascii_case_insensitive(a.ext(), b.ext())
                            .then_with(|| crate::utils::string::natural_cmp(a.clean(), b.clean())),
                        SortOrder::ExtensionReverse => cmp_ascii_case_insensitive(b.ext(), a.ext())
                            .then_with(|| crate::utils::string::natural_cmp(a.clean(), b.clean())),
                    };

                    if ord != std::cmp::Ordering::Equal {
                        return ord;
                    }
                    return b.score.cmp(&a.score);
                }

                if a.tier != b.tier {
                    return a.tier.cmp(&b.tier);
                }

                if a.tier < 2 {
                    let cmp = cmp_ascii_case_insensitive(a.clean(), b.clean());
                    if cmp != std::cmp::Ordering::Equal {
                        return cmp;
                    }
                }

                b.score.cmp(&a.score)
            });

            decorated
                .get(n as usize)
                .map(|d| d.item.data)
                .or_else(|| {
                    if is_query_empty && self.mode_index != 0 {
                        snapshot.get_item(n).map(|item| item.data)
                    } else {
                        snapshot.get_matched_item(n).map(|item| item.data)
                    }
                })
        } else if is_query_empty && self.mode_index != 0 {
            snapshot.get_item(n).map(|item| item.data)
        } else {
            snapshot.get_matched_item(n).map(|item| item.data)
        }
    }

    pub fn new_snapshot(nucleo: &mut nucleo::Nucleo<T>) -> (&nucleo::Snapshot<T>, Status) {
        let nucleo::Status { changed, running } = nucleo.tick(10);
        let snapshot = nucleo.snapshot();
        (
            snapshot,
            Status {
                item_count: snapshot.item_count(),
                matched_count: snapshot.matched_item_count(),
                running,
                changed,
            },
        )
    }

    pub fn raw_results(&self) -> impl ExactSizeIterator<Item = &T> + DoubleEndedIterator + '_ {
        let snapshot = self.nucleo.snapshot();
        snapshot.matched_items(..).map(|item| item.data)
    }

    /// matched item count, total item count
    pub fn counts(&self) -> (u32, u32) {
        let snapshot = self.nucleo.snapshot();
        (snapshot.matched_item_count(), snapshot.item_count())
    }

    pub fn set_stability(&mut self, threshold: crate::config::SortThreshold) {
        self.sort_threshold = threshold;
        let effective = threshold
            .get_effective_threshold(self.query.primary_column_query().unwrap_or_default());
        self.nucleo.set_stability(effective);
    }

    pub fn get_stability(&self) -> u32 {
        self.nucleo.get_stability()
    }

    pub fn restart(&mut self, clear_snapshot: bool) {
        self.nucleo.restart(clear_snapshot);
    }
}

#[derive(Debug, Default, Clone)]
pub struct Status {
    pub item_count: u32,
    pub matched_count: u32,
    pub running: bool,
    pub changed: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum WorkerError {
    #[error("the matcher injector has been shut down")]
    InjectorShutdown,
    #[error("{0}")]
    Custom(&'static str),
}

#[derive(Clone, Debug, PartialEq)]
pub enum GroupHeader {
    Named(Arc<str>),
    TierSeparator,
}

/// A vec of ItemResult, each ItemResult being the Group Header (if any), Column Texts of the Item, and Item
pub type WorkerResults<'a, T> = Vec<(Option<GroupHeader>, Vec<Text<'a>>, &'a T)>;

impl<T: SSS> Worker<T> {
    /// Returns:
    /// 1. Table of (Row, item, height)
    /// 2. Final column widths
    /// 3. Status
    ///
    /// # Notes
    /// - Final column width is at least header width
    pub fn results(
        &mut self,
        start: u32,
        end: u32,
        width_limits: &[u16],
        wrap: bool,
        max_height: usize,
        highlight_style: Style,
        matcher: &mut nucleo::Matcher,
        autoscroll: AutoscrollSettings,
        hscroll_offset: i8,
        vscroll: (u8, bool),
        show_skipped: bool,
        freeze_snapshot: bool,
    ) -> (WorkerResults<'_, T>, Vec<u16>, Vec<u16>, Status) {
        let (snapshot, status) = if freeze_snapshot {
            let snapshot = self.nucleo.snapshot();
            (
                snapshot,
                Status {
                    item_count: snapshot.item_count(),
                    matched_count: snapshot.matched_item_count(),
                    running: false,
                    changed: false,
                },
            )
        } else {
            Self::new_snapshot(&mut self.nucleo)
        };

        let mut widths = vec![0u16; self.columns.len()];
        let mut raw_widths = vec![vec![]; self.columns.len()];
        let total_width_limit: u16 = width_limits.iter().sum();
        let last_nonzero_idx = width_limits.iter().rposition(|&w| w != 0);
        let query_str = self.query.primary_column_query().unwrap_or_default();
        let is_query_empty = query_str.is_empty();
        let query_len = query_str.len();
        let effective_dir_first = self.dir_first && self.mode_index == 0;
        let effective_sort_order = if self.mode_index == 0 {
            self.sort_order
        } else {
            None
        };
        let should_sort = effective_sort_order.is_some()
            || (!is_query_empty
                && ((self.frecency && self.frecency_snapshot.is_some())
                    || (self.depth_penalty > 0 && self.mode_index == 0)))
            || effective_dir_first;

        let (items_buf, initial_prev_tier) = if should_sort {
            let total = status.matched_count;
            let total_sort = if effective_sort_order.is_some() {
                total
            } else if self.sort_cap > 0 {
                total.min(self.sort_cap as u32)
            } else {
                total
            };
            let mut items: Vec<_> = snapshot.matched_items(0..total_sort).enumerate().collect();
            let penalty = if is_query_empty || self.mode_index != 0 {
                0
            } else {
                self.depth_penalty
            };
            let frec_weight = self.frecency_weight;
            let snapshot_ref = if self.frecency {
                self.frecency_snapshot.as_ref()
            } else {
                None
            };
            let col0 = &self.columns[0];
            let scan_end = total.min(total_sort + 5000);
            if scan_end > total_sort {
                for (idx, item) in snapshot.matched_items(total_sort..scan_end).enumerate() {
                    let raw_path = col0.raw(item.data);
                    let has_frecency =
                        snapshot_ref.map_or(false, |snap| snap.has_bonus_fast(raw_path.as_ref()));
                    let is_direct = if effective_dir_first {
                        let (tier, _) = get_item_tier_and_clean_path(raw_path.as_ref(), true);
                        tier < 2
                    } else {
                        false
                    };
                    if has_frecency || is_direct {
                        items.push((total_sort as usize + idx, item));
                    }
                }
            }
            struct DecoratedItem<'a, T> {
                item: nucleo::Item<'a, T>,
                tier: u8,
                raw_path: Cow<'a, str>,
                clean_range: (usize, usize),
                score: u64,
                mtime: Option<std::time::SystemTime>,
                btime: Option<std::time::SystemTime>,
                size: Option<u64>,
                ext_range: Option<(usize, usize)>,
            }

            impl<'a, T> DecoratedItem<'a, T> {
                #[inline]
                fn clean(&self) -> &str {
                    &self.raw_path[self.clean_range.0..self.clean_range.1]
                }

                #[inline]
                fn ext(&self) -> &str {
                    if let Some((start, end)) = self.ext_range {
                        &self.raw_path[start..end]
                    } else {
                        ""
                    }
                }
            }

            let mut decorated: Vec<DecoratedItem<'_, T>> = items
                .into_iter()
                .map(|(idx, item)| {
                    let raw_path = col0.raw(item.data);
                    let score = compute_item_score(
                        total,
                        idx,
                        raw_path.as_ref(),
                        is_query_empty,
                        query_len,
                        snapshot_ref,
                        frec_weight,
                        self.location_bias,
                        penalty,
                    );
                    let (tier, clean) =
                        get_item_tier_and_clean_path(raw_path.as_ref(), effective_dir_first);
                    let clean_start = clean.as_ptr() as usize - raw_path.as_ref().as_ptr() as usize;
                    let clean_range = (clean_start, clean_start + clean.len());
                    let (mtime, btime, size, ext_range) = match effective_sort_order {
                        Some(
                            crate::action::SortOrder::Modified
                            | crate::action::SortOrder::ModifiedReverse,
                        ) => {
                            let m = std::fs::metadata(clean)
                                .or_else(|_| std::fs::symlink_metadata(clean))
                                .and_then(|meta| meta.modified())
                                .ok();
                            (m, None, None, None)
                        }
                        Some(
                            crate::action::SortOrder::Created
                            | crate::action::SortOrder::CreatedReverse,
                        ) => {
                            let meta = std::fs::metadata(clean)
                                .or_else(|_| std::fs::symlink_metadata(clean))
                                .ok();
                            let b = meta
                                .as_ref()
                                .and_then(|m| m.created().ok())
                                .or_else(|| meta.as_ref().and_then(|m| m.modified().ok()));
                            (None, b, None, None)
                        }
                        Some(
                            crate::action::SortOrder::Size | crate::action::SortOrder::SizeReverse,
                        ) => {
                            let s = std::fs::metadata(clean)
                                .or_else(|_| std::fs::symlink_metadata(clean))
                                .map(|meta| meta.len())
                                .ok();
                            (None, None, s, None)
                        }
                        Some(
                            crate::action::SortOrder::Extension
                            | crate::action::SortOrder::ExtensionReverse,
                        ) => {
                            let ext = std::path::Path::new(clean)
                                .extension()
                                .and_then(|e| e.to_str());
                            let range = ext.map(|e| {
                                let start =
                                    e.as_ptr() as usize - raw_path.as_ref().as_ptr() as usize;
                                (start, start + e.len())
                            });
                            (None, None, None, range)
                        }
                        _ => (None, None, None, None),
                    };
                    DecoratedItem {
                        item,
                        tier,
                        raw_path,
                        clean_range,
                        score,
                        mtime,
                        btime,
                        size,
                        ext_range,
                    }
                })
                .collect();

            decorated.sort_unstable_by(|a, b| {
                if let Some(sort_order) = effective_sort_order {
                    use crate::action::SortOrder;
                    if effective_dir_first && a.tier != b.tier {
                        return a.tier.cmp(&b.tier);
                    }

                    let ord = match sort_order {
                        SortOrder::Alphabetical => cmp_ascii_case_insensitive(a.clean(), b.clean())
                            .then_with(|| a.clean().cmp(b.clean())),
                        SortOrder::AlphabeticalReverse => {
                            cmp_ascii_case_insensitive(b.clean(), a.clean())
                                .then_with(|| b.clean().cmp(a.clean()))
                        }
                        SortOrder::Natural => {
                            crate::utils::string::natural_cmp(a.clean(), b.clean())
                                .then_with(|| a.clean().cmp(b.clean()))
                        }
                        SortOrder::NaturalReverse => {
                            crate::utils::string::natural_cmp(b.clean(), a.clean())
                                .then_with(|| b.clean().cmp(a.clean()))
                        }
                        SortOrder::Modified => {
                            let a_time = a.mtime.unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                            let b_time = b.mtime.unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                            a_time.cmp(&b_time).then_with(|| {
                                crate::utils::string::natural_cmp(a.clean(), b.clean())
                            })
                        }
                        SortOrder::ModifiedReverse => {
                            let a_time = a.mtime.unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                            let b_time = b.mtime.unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                            b_time.cmp(&a_time).then_with(|| {
                                crate::utils::string::natural_cmp(a.clean(), b.clean())
                            })
                        }
                        SortOrder::Created => {
                            let a_time = a.btime.unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                            let b_time = b.btime.unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                            a_time.cmp(&b_time).then_with(|| {
                                crate::utils::string::natural_cmp(a.clean(), b.clean())
                            })
                        }
                        SortOrder::CreatedReverse => {
                            let a_time = a.btime.unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                            let b_time = b.btime.unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                            b_time.cmp(&a_time).then_with(|| {
                                crate::utils::string::natural_cmp(a.clean(), b.clean())
                            })
                        }
                        SortOrder::Size => {
                            let a_size = a.size.unwrap_or(0);
                            let b_size = b.size.unwrap_or(0);
                            a_size.cmp(&b_size).then_with(|| {
                                crate::utils::string::natural_cmp(a.clean(), b.clean())
                            })
                        }
                        SortOrder::SizeReverse => {
                            let a_size = a.size.unwrap_or(0);
                            let b_size = b.size.unwrap_or(0);
                            b_size.cmp(&a_size).then_with(|| {
                                crate::utils::string::natural_cmp(a.clean(), b.clean())
                            })
                        }
                        SortOrder::Extension => cmp_ascii_case_insensitive(a.ext(), b.ext())
                            .then_with(|| crate::utils::string::natural_cmp(a.clean(), b.clean())),
                        SortOrder::ExtensionReverse => cmp_ascii_case_insensitive(b.ext(), a.ext())
                            .then_with(|| crate::utils::string::natural_cmp(a.clean(), b.clean())),
                    };

                    if ord != std::cmp::Ordering::Equal {
                        return ord;
                    }
                    return b.score.cmp(&a.score);
                }

                if a.tier != b.tier {
                    return a.tier.cmp(&b.tier);
                }

                if a.tier < 2 {
                    let cmp = cmp_ascii_case_insensitive(a.clean(), b.clean());
                    if cmp != std::cmp::Ordering::Equal {
                        return cmp;
                    }
                }

                b.score.cmp(&a.score)
            });

            let range_start = start.min(total) as usize;
            let range_end = end.min(total) as usize;
            let prev_tier = if range_start > 0 && range_start <= decorated.len() {
                decorated.get(range_start - 1).map(|d| d.tier)
            } else {
                None
            };
            let items: Vec<_> = if range_start < decorated.len() {
                let take_count = range_end.saturating_sub(range_start);
                decorated
                    .into_iter()
                    .skip(range_start)
                    .take(take_count)
                    .map(|d| (d.item, d.tier))
                    .collect()
            } else {
                snapshot
                    .matched_items(start.min(status.matched_count)..end.min(status.matched_count))
                    .map(|item| {
                        let tier = if effective_dir_first {
                            let raw_path = col0.raw(item.data);
                            let (tier, _) = get_item_tier_and_clean_path(raw_path.as_ref(), true);
                            tier
                        } else {
                            2
                        };
                        (item, tier)
                    })
                    .collect()
            };
            (items, prev_tier)
        } else if is_query_empty && self.mode_index != 0 {
            let items: Vec<_> = (start.min(status.matched_count)..end.min(status.matched_count))
                .filter_map(|idx| snapshot.get_item(idx).map(|item| (item, 2u8)))
                .collect();
            (items, None)
        } else {
            let col0 = &self.columns[0];
            let items: Vec<_> = snapshot
                .matched_items(start.min(status.matched_count)..end.min(status.matched_count))
                .map(|item| {
                    let tier = if effective_dir_first {
                        let raw_path = col0.raw(item.data);
                        let (tier, _) = get_item_tier_and_clean_path(raw_path.as_ref(), true);
                        tier
                    } else {
                        2
                    };
                    (item, tier)
                })
                .collect();
            (items, None)
        };

        let (vscroll_offset, stacked) = vscroll;

        let mut table = Vec::new();
        let mut last_emitted_group: Option<Arc<str>> = None;
        let mut last_tier: Option<u8> = initial_prev_tier;
        let group_header = &self.group_header;

        for (item, item_tier) in &items_buf {
            let mut row = vec![];

            let mut to_skip = vscroll_offset as usize;
            let mut skip = !show_skipped;
            for (i, c) in self.columns.iter().enumerate() {
                let mut t = c.format(item.data);
                if stacked {
                    if to_skip >= t.height() {
                        to_skip -= t.height();
                        t.lines.clear();
                    } else {
                        skip = false;
                        t.lines.drain(..to_skip);
                        to_skip = 0;
                        if max_height > 0 && t.height() > max_height {
                            t.lines.truncate(max_height);
                            if let Some(last_line) = t.lines.last_mut() {
                                last_line.spans.push(truncation_indicator());
                            }
                        }
                    }
                } else {
                    if t.height() > to_skip {
                        skip = false;
                    }
                    t.lines.drain(..to_skip);
                    if max_height > 0 && t.height() > max_height {
                        t.lines.truncate(max_height);
                        if let Some(last_line) = t.lines.last_mut() {
                            last_line.spans.push(truncation_indicator());
                        }
                    }

                    if width_limits.get(i).cloned() != Some(0) && !skip {
                        raw_widths[i].push(t.width() as u16);
                    }
                }
                row.push(t);
            }
            if skip {
                continue;
            }

            let row: Vec<Text> = row
                .into_iter()
                .enumerate()
                .zip(width_limits.iter().chain(std::iter::repeat(&u16::MAX)))
                .map(|((col_idx, cell), &width_limit)| {
                    let column = &self.columns[col_idx];

                    let effective_limit = if Some(col_idx) == last_nonzero_idx {
                        total_width_limit.saturating_sub(width_limits.iter().take(col_idx).sum())
                    } else {
                        width_limit
                    };

                    let (cell, computed_width) = if effective_limit == 0 {
                        (Default::default(), 0)
                    } else if column.filter {
                        render_cell(
                            cell,
                            col_idx,
                            snapshot,
                            item,
                            matcher,
                            highlight_style,
                            wrap,
                            effective_limit,
                            &mut self.col_indices_buffer,
                            autoscroll.clone(),
                            hscroll_offset,
                        )
                    } else if wrap {
                        let (cell, wrapped) = wrap_text(cell, effective_limit);

                        let width = if wrapped {
                            effective_limit as usize
                        } else {
                            cell.width()
                        };
                        (cell, width)
                    } else {
                        let width = cell.width();
                        (cell, width)
                    };

                    if col_idx < widths.len() {
                        widths[col_idx] = widths[col_idx].max(computed_width as u16)
                    }

                    cell
                })
                .collect();

            let mut header_to_emit = None;
            if let Some(f) = group_header
                && let Some(group) = f(item.data)
            {
                if Some(&group) != last_emitted_group.as_ref() {
                    header_to_emit = Some(GroupHeader::Named(group.clone()));
                    last_emitted_group = Some(group);
                }
            } else if effective_dir_first {
                if let Some(last) = last_tier {
                    if *item_tier != last {
                        header_to_emit = Some(GroupHeader::TierSeparator);
                    }
                }
                last_tier = Some(*item_tier);
            }

            table.push((header_to_emit, row, item.data));
        }

        // Nonempty columns should have width at least their header
        for (w, c) in widths.iter_mut().zip(self.columns.iter()) {
            let name_width = c.name.width() as u16;
            if *w != 0 {
                *w = (*w).max(name_width);
            }
        }

        let medians = raw_widths
            .into_iter()
            .map(|mut v| {
                if v.is_empty() {
                    0
                } else {
                    v.sort_unstable();
                    v[v.len() / 2]
                }
            })
            .collect();

        (table, widths, medians, status)
    }

    pub fn exact_column_match(&mut self, column: &str) -> Option<&T> {
        let (i, col) = self
            .columns
            .iter()
            .enumerate()
            .find(|(_, c)| column == &*c.name)?;

        let query = self.query.get(column).map(|s| &**s).or_else(|| {
            self.column_options[i]
                .contains(ColumnOptions::OrUseDefault)
                .then(|| self.query.primary_column_query())
                .flatten()
        })?;

        let snapshot = self.nucleo.snapshot();
        snapshot.matched_items(..).find_map(|item| {
            let content = col.raw(item.data);
            if content.as_str() == query {
                Some(item.data)
            } else {
                None
            }
        })
    }

    pub fn format_with<'a>(&'a self, item: &'a T, col: &str) -> Option<Cow<'a, str>> {
        self.columns
            .iter()
            .find(|c| &*c.name == col)
            .map(|c| c.raw(item))
    }
}

fn render_cell<T: SSS>(
    cell: Text<'_>,
    col_idx: usize,
    snapshot: &nucleo::Snapshot<T>,
    item: &nucleo::Item<T>,
    matcher: &mut nucleo::Matcher,
    highlight_style: Style,
    wrap: bool,
    width_limit: u16,
    col_indices_buffer: &mut Vec<u32>,
    mut autoscroll: AutoscrollSettings,
    hscroll_offset: i8,
) -> (Text<'static>, usize) {
    if !autoscroll.always {
        autoscroll.enabled &= !wrap;
    }

    let mut cell_width = 0;
    let mut wrapped = false;

    // get indices
    let indices_buffer = col_indices_buffer;
    indices_buffer.clear();
    snapshot.pattern().column_pattern(col_idx).indices(
        item.matcher_columns[col_idx].slice(..),
        matcher,
        indices_buffer,
    );
    indices_buffer.sort_unstable();
    indices_buffer.dedup();
    let mut indices = indices_buffer.drain(..);

    let mut lines = vec![];
    let mut next_highlight_idx = indices.next().unwrap_or(u32::MAX);
    let mut grapheme_idx = 0u32;

    let mut line_graphemes = Vec::new();

    for line in &cell {
        // 1: Collect graphemes, compute styles, and find the relevant match on this line.
        line_graphemes.clear();
        let mut match_idx = None;

        for span in line {
            // this looks like a bug on first glance, we are iterating
            // graphemes but treating them as char indices. The reason that
            // this is correct is that nucleo will only ever consider the first char
            // of a grapheme (and discard the rest of the grapheme) so the indices
            // returned by nucleo are essentially grapheme indecies
            for grapheme in span.content.graphemes(true) {
                let is_match = grapheme_idx == next_highlight_idx;

                let style = if is_match {
                    next_highlight_idx = indices.next().unwrap_or(u32::MAX);
                    span.style.patch(highlight_style)
                } else {
                    span.style
                };

                if is_match && (autoscroll.end || match_idx.is_none()) {
                    match_idx = Some(line_graphemes.len());
                }

                line_graphemes.push((grapheme, style));
                grapheme_idx += 1;
            }
        }

        // 2: Calculate where to start rendering this line
        let mut i; // start_idx

        if autoscroll.enabled && autoscroll.end {
            i = match_idx.unwrap_or(line_graphemes.len().saturating_sub(1));

            let preserved_width = line_graphemes
                [..autoscroll.initial_preserved.min(line_graphemes.len())]
                .iter()
                .map(|(g, _)| g.width())
                .sum::<usize>();

            let target_width = if let Some(x) = match_idx {
                (width_limit as usize)
                    .saturating_sub(autoscroll.context.min(line_graphemes.len() - x - 1))
            } else {
                width_limit as usize
            }
            .saturating_sub(preserved_width);

            let mut current_width = 0;

            while i > autoscroll.initial_preserved {
                let w = line_graphemes[i - 1].0.width();
                let indicator_width = if i - 1 > autoscroll.initial_preserved {
                    autoscroll.indicator.as_str().width()
                } else {
                    0
                };

                if current_width + w + indicator_width < target_width {
                    i -= 1;
                    current_width += w;
                } else {
                    break;
                }
            }

            i = i.saturating_add_signed(hscroll_offset as isize);

            if i <= autoscroll.initial_preserved {
                i = 0;
            }
        } else if autoscroll.enabled
            && let Some(m_idx) = match_idx
        {
            i = (m_idx as i32 - autoscroll.context as i32).max(0) as usize;

            let mut tail_width: usize = line_graphemes[i..].iter().map(|(g, _)| g.width()).sum();

            let preserved_width = line_graphemes
                [..autoscroll.initial_preserved.min(line_graphemes.len())]
                .iter()
                .map(|(g, _)| g.width())
                .sum::<usize>();

            // Expand leftwards as long as the total rendered width <= width_limit
            while i > autoscroll.initial_preserved {
                let prev_width = line_graphemes[i - 1].0.width();
                // Only reserve space for "..." if we aren't reaching the very start
                let indicator_width = if i - 1 > autoscroll.initial_preserved {
                    autoscroll.indicator.as_str().width()
                } else {
                    0
                };

                if tail_width + preserved_width + indicator_width + prev_width
                    <= width_limit as usize
                {
                    i -= 1;
                    tail_width += prev_width;
                } else {
                    break;
                }
            }

            i = i.saturating_add_signed(hscroll_offset as isize);

            if i <= autoscroll.initial_preserved {
                i = 0;
            }
        } else {
            i = hscroll_offset.max(0) as usize;
        };

        // 3: Apply the standard wrapping and Span generation logic to the visible slice
        let mut current_spans = Vec::new();
        let mut current_span = String::new();
        let mut current_style = Style::default();
        let mut current_width = 0;

        // Add preserved prefix and ellipsis if needed
        if i > 0 && autoscroll.enabled {
            for (g, s) in
                line_graphemes.drain(..autoscroll.initial_preserved.min(line_graphemes.len()))
            {
                if s != current_style {
                    if !current_span.is_empty() {
                        current_spans.push(Span::styled(current_span, current_style));
                    }
                    current_span = String::new();
                    current_style = s;
                }
                current_span.push_str(g);
            }
            if !current_span.is_empty() {
                current_spans.push(Span::styled(current_span, current_style));
            }
            i -= autoscroll.initial_preserved;

            current_width += current_spans.iter().map(|x| x.width()).sum::<usize>();
            current_spans.push(Span::styled(
                autoscroll.indicator.clone(),
                Style::from(autoscroll.indicator_style),
            ));
            current_width += autoscroll.indicator.as_str().width();

            current_span = String::new();
            current_style = Style::default();
        }

        // prevent stuck invisible columns
        if !line_graphemes.is_empty() {
            cell_width = cell_width.max(1);
            i = i.min(line_graphemes.len())
        }

        let mut graphemes = line_graphemes.drain(i..);

        while let Some((mut grapheme, mut style)) = graphemes.next() {
            if current_width + grapheme.width() > width_limit as usize {
                if !current_span.is_empty() {
                    current_spans.push(Span::styled(current_span, current_style));
                    current_span = String::new();
                }
                if wrap {
                    current_spans.push(wrapping_indicator());
                    lines.push(Line::from(take(&mut current_spans)));

                    current_width = 0;
                    wrapped = true;
                } else {
                    break;
                }
            } else if current_width + grapheme.width() == width_limit as usize {
                if wrap {
                    let mut new = grapheme.to_string();
                    if current_style != style {
                        current_spans.push(Span::styled(take(&mut current_span), current_style));
                        current_style = style;
                    };
                    while let Some((grapheme2, style2)) = graphemes.next() {
                        if grapheme2.width() == 0 {
                            new.push_str(grapheme2);
                        } else {
                            if !current_span.is_empty() {
                                current_spans.push(Span::styled(current_span, current_style));
                            }
                            current_spans.push(wrapping_indicator());
                            lines.push(Line::from(take(&mut current_spans)));

                            // new line starts from last char
                            current_span = new.clone(); // rust can't tell that clone is unnecessary here
                            current_width = grapheme.width();
                            wrapped = true;

                            grapheme = grapheme2;
                            style = style2;
                            break; // continue normal processing
                        }
                    }
                    if !wrapped {
                        current_span.push_str(&new);
                        // we reached the end of the line exactly, end line
                        current_spans.push(Span::styled(take(&mut current_span), style));
                        current_style = style;
                        current_width += grapheme.width();
                        break;
                    }
                } else {
                    if style != current_style {
                        if !current_span.is_empty() {
                            current_spans.push(Span::styled(current_span, current_style));
                        }
                        current_span = String::new();
                        current_style = style;
                    }
                    current_span.push_str(grapheme);
                    current_width += grapheme.width();
                    break;
                }
            }

            // normal processing
            if style != current_style {
                if !current_span.is_empty() {
                    current_spans.push(Span::styled(current_span, current_style))
                }
                current_span = String::new();
                current_style = style;
            }
            current_span.push_str(grapheme);
            current_width += grapheme.width();
        }

        current_spans.push(Span::styled(current_span, current_style));
        lines.push(Line::from(current_spans));
        cell_width = cell_width.max(current_width);

        grapheme_idx += 1; // newline
    }

    (
        Text::from(lines),
        if wrapped {
            width_limit as usize
        } else {
            cell_width
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use nucleo::{Matcher, Nucleo};
    use ratatui::style::{Color, Style};
    use ratatui::text::Text;
    use std::sync::Arc;

    /// Sets up the necessary Nucleo state to trigger a match
    fn setup_nucleo_mocks(
        search_query: &str,
        item_text: &str,
    ) -> (Nucleo<String>, Matcher, Vec<u32>) {
        let mut nucleo = Nucleo::<String>::new(nucleo::Config::DEFAULT, Arc::new(|| {}), None, 1);

        let injector = nucleo.injector();
        injector.push(item_text.to_string(), |item, columns| {
            columns[0] = item.clone().into();
        });

        nucleo.pattern.reparse(
            0,
            search_query,
            nucleo::pattern::CaseMatching::Ignore,
            nucleo::pattern::Normalization::Smart,
            false,
        );

        nucleo.tick(10); // Process the item

        let matcher = Matcher::default();
        let buffer = Vec::new();

        (nucleo, matcher, buffer)
    }

    #[test]
    fn test_no_scroll_context_renders_normally() {
        let (nucleo, mut matcher, mut buffer) = setup_nucleo_mocks("match", "hello match world");
        let snapshot = nucleo.snapshot();
        let item = snapshot.get_item(0).unwrap();

        let cell = Text::from("hello match world");
        let highlight = Style::default().fg(Color::Red);

        let (result_text, width) = render_cell(
            cell,
            0,
            &snapshot,
            &item,
            &mut matcher,
            highlight,
            false,
            u16::MAX,
            &mut buffer,
            AutoscrollSettings {
                enabled: false,
                ..Default::default()
            },
            0,
        );

        let output_str = result_text.to_string();
        assert_eq!(output_str, "hello match world");
        assert_eq!(width, 17);
    }

    #[test]
    fn test_scroll_context_cuts_prefix_correctly() {
        let (nucleo, mut matcher, mut buffer) = setup_nucleo_mocks("match", "hello match world");
        let snapshot = nucleo.snapshot();
        let item = snapshot.get_item(0).unwrap();

        let cell = Text::from("hello match world");
        let highlight = Style::default().fg(Color::Red);

        let (result_text, _) = render_cell(
            cell,
            0,
            &snapshot,
            &item,
            &mut matcher,
            highlight,
            false,
            u16::MAX,
            &mut buffer,
            AutoscrollSettings {
                initial_preserved: 0,
                context: 2,
                ..Default::default()
            },
            0,
        );

        let output_str = result_text.to_string();
        assert_eq!(output_str, "hello match world");
    }

    #[test]
    fn test_scroll_context_backfills_to_fill_width_limit() {
        // Query "match". Starts at index 10.
        // "abcdefghijmatch"
        // autoscroll = Some((preserved=0, context=1))
        // initial_start_idx = 10 + 0 - 1 = 9 ("jmatch").
        // width_limit = 10.
        // tail_width ("jmatch") = 6.
        // Try to decrease start_idx.
        // start_idx=8 ("ijmatch"), tail_width=7.
        // start_idx=7 ("hijmatch"), tail_width=8.
        // start_idx=6 ("ghijmatch"), tail_width=9.
        // start_idx=5 ("fghijmatch"), tail_width=10.
        // start_idx=4 ("efghijmatch"), tail_width=11 > 10 (STOP).
        // Result start_idx = 5. Output: "fghijmatch"

        let (nucleo, mut matcher, mut buffer) = setup_nucleo_mocks("match", "abcdefghijmatch");
        let snapshot = nucleo.snapshot();
        let item = snapshot.get_item(0).unwrap();

        let cell = Text::from("abcdefghijmatch");
        let highlight = Style::default().fg(Color::Red);

        let (result_text, width) = render_cell(
            cell,
            0,
            &snapshot,
            &item,
            &mut matcher,
            highlight,
            false,
            10,
            &mut buffer,
            AutoscrollSettings {
                initial_preserved: 0,
                context: 1,
                indicator: "…".to_string(),
                ..Default::default()
            },
            0,
        );

        let output_str = result_text.to_string();
        assert_eq!(output_str, "…ghijmatch");
        assert_eq!(width, 10);
    }

    #[test]
    fn test_preserved_prefix_and_ellipsis() {
        // Query "match". Starts at index 10.
        // "abcdefghijmatch"
        // autoscroll = Some((preserved=3, context=1))
        // initial_start_idx = 10 + 0 - 1 = 9.
        // start_idx = 9.
        // width_limit = 10.
        // preserved_width ("abc") = 3.
        // gap_indicator_width ("…") = 1.
        // tail_width ("jmatch") = 6.
        // total = 3 + 1 + 6 = 10.
        // start_idx=9, preserved=3. 9 > 3 + 1 (9 > 4) -> preserved_prefix = "abc", output: "abc…jmatch"

        let (nucleo, mut matcher, mut buffer) = setup_nucleo_mocks("match", "abcdefghijmatch");
        let snapshot = nucleo.snapshot();
        let item = snapshot.get_item(0).unwrap();

        let cell = Text::from("abcdefghijmatch");
        let highlight = Style::default().fg(Color::Red);

        let (result_text, width) = render_cell(
            cell,
            0,
            &snapshot,
            &item,
            &mut matcher,
            highlight,
            false,
            10,
            &mut buffer,
            AutoscrollSettings {
                initial_preserved: 3,
                context: 1,
                indicator: "…".to_string(),
                ..Default::default()
            },
            0,
        );

        let output_str = result_text.to_string();
        assert_eq!(output_str, "abc…jmatch");
        assert_eq!(width, 10);
    }

    #[test]
    fn test_wrap() {
        let (nucleo, mut matcher, mut buffer) = setup_nucleo_mocks("match", "abcdefmatch");
        let snapshot = nucleo.snapshot();
        let item = snapshot.get_item(0).unwrap();

        let cell = Text::from("abcdefmatch");
        let highlight = Style::default().fg(Color::Red);

        let (result_text, width) = render_cell(
            cell,
            0,
            &snapshot,
            &item,
            &mut matcher,
            highlight,
            true,
            10,
            &mut buffer,
            AutoscrollSettings {
                initial_preserved: 3,
                context: 1,
                ..Default::default()
            },
            -2,
        );

        let output_str = result_text.to_string();
        assert_eq!(output_str, "abcdefmat↵\nch");
        assert_eq!(width, 10);
    }

    #[test]
    fn test_wrap_edge_case_6_chars_width_5() {
        let (nucleo, mut matcher, mut buffer) = setup_nucleo_mocks("", "123456");
        let snapshot = nucleo.snapshot();
        let item = snapshot.get_item(0).unwrap();

        let cell = Text::from("123456");
        let highlight = Style::default().fg(Color::Red);

        let (result_text, width) = render_cell(
            cell,
            0,
            &snapshot,
            &item,
            &mut matcher,
            highlight,
            true,
            5,
            &mut buffer,
            AutoscrollSettings {
                enabled: false,
                ..Default::default()
            },
            0,
        );

        let output_str = result_text.to_string();
        // Expecting "1234↵" and "56"
        assert_eq!(output_str, "1234↵\n56");
        assert_eq!(width, 5);
    }

    #[test]
    fn test_autoscroll_end() {
        let (nucleo, mut matcher, mut buffer) = setup_nucleo_mocks("match", "abcdefghijmatch");
        let snapshot = nucleo.snapshot();
        let item = snapshot.get_item(0).unwrap();

        let cell = Text::from("abcdefghijmatch");
        let highlight = Style::default().fg(Color::Red);

        let (result_text, width) = render_cell(
            cell,
            0,
            &snapshot,
            &item,
            &mut matcher,
            highlight,
            false,
            10,
            &mut buffer,
            AutoscrollSettings {
                end: true,
                context: 4,
                indicator: "…".to_string(),
                ..Default::default()
            },
            0,
        );

        let output_str = result_text.to_string();
        assert_eq!(output_str, "…ghijmatch");
        assert_eq!(width, 10);
    }

    #[test]
    fn test_autoscroll_end_context() {
        let (nucleo, mut matcher, mut buffer) = setup_nucleo_mocks("ma", "abcdefghijmatch");
        let snapshot = nucleo.snapshot();
        let item = snapshot.get_item(0).unwrap();

        let cell = Text::from("abcdefghijmatch");
        let highlight = Style::default().fg(Color::Red);

        let (result_text, width) = render_cell(
            cell,
            0,
            &snapshot,
            &item,
            &mut matcher,
            highlight,
            false,
            10,
            &mut buffer,
            AutoscrollSettings {
                end: true,
                context: 2,
                indicator: "…".to_string(),
                ..Default::default()
            },
            0,
        );

        let output_str = result_text.to_string();
        assert_eq!(output_str, "…fghijmatc");
        assert_eq!(width, 10);
    }

    #[test]
    fn test_tier_separators_emitted_on_tier_boundaries() {
        let mut worker = Worker::<String>::new_single_column();
        worker.dir_first = true;

        let injector = worker.nucleo.injector();
        // Tier 0: Direct dirs (ends with /)
        injector.push("alpha/".to_string(), |item, cols| {
            cols[0] = item.clone().into();
        });
        injector.push("beta/".to_string(), |item, cols| {
            cols[0] = item.clone().into();
        });
        // Tier 1: Direct files (no /)
        injector.push("file_a.txt".to_string(), |item, cols| {
            cols[0] = item.clone().into();
        });
        injector.push("file_b.txt".to_string(), |item, cols| {
            cols[0] = item.clone().into();
        });
        // Tier 2: Deep items (has / in middle)
        injector.push("sub/deep_file.txt".to_string(), |item, cols| {
            cols[0] = item.clone().into();
        });

        worker.nucleo.tick(10);

        let mut matcher = Matcher::default();
        let (results, _, _, _) = worker.results(
            0,
            10,
            &[100],
            false,
            0,
            Style::default(),
            &mut matcher,
            AutoscrollSettings::default(),
            0,
            (0, false),
            true,
            false,
        );

        assert_eq!(results.len(), 5);
        // Item 0: Tier 0 ("alpha/") - first item overall -> None
        assert_eq!(results[0].0, None);
        assert_eq!(results[0].2, "alpha/");

        // Item 1: Tier 0 ("beta/") - same tier -> None
        assert_eq!(results[1].0, None);
        assert_eq!(results[1].2, "beta/");

        // Item 2: Tier 1 ("file_a.txt") - tier boundary 0 -> 1 -> Some(TierSeparator)
        assert_eq!(results[2].0, Some(GroupHeader::TierSeparator));
        assert_eq!(results[2].2, "file_a.txt");

        // Item 3: Tier 1 ("file_b.txt") - same tier -> None
        assert_eq!(results[3].0, None);
        assert_eq!(results[3].2, "file_b.txt");

        // Item 4: Tier 2 ("sub/deep_file.txt") - tier boundary 1 -> 2 -> Some(TierSeparator)
        assert_eq!(results[4].0, Some(GroupHeader::TierSeparator));
        assert_eq!(results[4].2, "sub/deep_file.txt");
    }

    #[test]
    fn test_tier_separators_with_scrolling() {
        let mut worker = Worker::<String>::new_single_column();
        worker.dir_first = true;

        let injector = worker.nucleo.injector();
        injector.push("dir/".to_string(), |item, cols| {
            cols[0] = item.clone().into();
        });
        injector.push("file.txt".to_string(), |item, cols| {
            cols[0] = item.clone().into();
        });
        injector.push("deep/item.txt".to_string(), |item, cols| {
            cols[0] = item.clone().into();
        });

        worker.nucleo.tick(10);

        let mut matcher = Matcher::default();
        // Query window starting at index 1 ("file.txt")
        let (results, _, _, _) = worker.results(
            1,
            3,
            &[100],
            false,
            0,
            Style::default(),
            &mut matcher,
            AutoscrollSettings::default(),
            0,
            (0, false),
            true,
            false,
        );

        assert_eq!(results.len(), 2);
        // Item 0 in slice is "file.txt" (Tier 1). Previous item in dataset was "dir/" (Tier 0).
        // Since tier changed from 0 to 1, TierSeparator must be emitted!
        assert_eq!(results[0].0, Some(GroupHeader::TierSeparator));
        assert_eq!(results[0].2, "file.txt");

        // Item 1 in slice is "deep/item.txt" (Tier 2). Previous item was Tier 1.
        // TierSeparator must be emitted!
        assert_eq!(results[1].0, Some(GroupHeader::TierSeparator));
        assert_eq!(results[1].2, "deep/item.txt");
    }

    #[test]
    fn test_tier_separators_disabled_when_dir_first_is_false() {
        let mut worker = Worker::<String>::new_single_column();
        worker.dir_first = false;

        let injector = worker.nucleo.injector();
        injector.push("dir/".to_string(), |item, cols| {
            cols[0] = item.clone().into();
        });
        injector.push("file.txt".to_string(), |item, cols| {
            cols[0] = item.clone().into();
        });

        worker.nucleo.tick(10);

        let mut matcher = Matcher::default();
        let (results, _, _, _) = worker.results(
            0,
            10,
            &[100],
            false,
            0,
            Style::default(),
            &mut matcher,
            AutoscrollSettings::default(),
            0,
            (0, false),
            true,
            false,
        );

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, None);
        assert_eq!(results[1].0, None);
    }

    #[test]
    fn test_worker_sort_orders() {
        use crate::action::SortOrder;

        let mut worker = Worker::<String>::new_single_column();
        let injector = worker.nucleo.injector();
        injector.push("file10.txt".to_string(), |item, cols| {
            cols[0] = item.clone().into();
        });
        injector.push("file2.txt".to_string(), |item, cols| {
            cols[0] = item.clone().into();
        });
        injector.push("file1.txt".to_string(), |item, cols| {
            cols[0] = item.clone().into();
        });
        injector.push("a.zip".to_string(), |item, cols| {
            cols[0] = item.clone().into();
        });
        injector.push("b.tar".to_string(), |item, cols| {
            cols[0] = item.clone().into();
        });

        worker.nucleo.set_stability(u32::MAX);
        worker.nucleo.tick(10);
        let mut matcher = Matcher::default();

        // 1. Alphabetical
        worker.set_sort_order(Some(SortOrder::Alphabetical));
        let (results, _, _, _) = worker.results(
            0,
            10,
            &[100],
            false,
            0,
            Style::default(),
            &mut matcher,
            AutoscrollSettings::default(),
            0,
            (0, false),
            true,
            false,
        );
        let items: Vec<&String> = results.iter().map(|r| r.2).collect();
        assert_eq!(
            items,
            vec!["a.zip", "b.tar", "file1.txt", "file10.txt", "file2.txt"]
        );

        // 2. Alphabetical Reverse
        worker.set_sort_order(Some(SortOrder::AlphabeticalReverse));
        let (results, _, _, _) = worker.results(
            0,
            10,
            &[100],
            false,
            0,
            Style::default(),
            &mut matcher,
            AutoscrollSettings::default(),
            0,
            (0, false),
            true,
            false,
        );
        let items: Vec<&String> = results.iter().map(|r| r.2).collect();
        assert_eq!(
            items,
            vec!["file2.txt", "file10.txt", "file1.txt", "b.tar", "a.zip"]
        );

        // 3. Natural
        worker.set_sort_order(Some(SortOrder::Natural));
        let (results, _, _, _) = worker.results(
            0,
            10,
            &[100],
            false,
            0,
            Style::default(),
            &mut matcher,
            AutoscrollSettings::default(),
            0,
            (0, false),
            true,
            false,
        );
        let items: Vec<&String> = results.iter().map(|r| r.2).collect();
        assert_eq!(
            items,
            vec!["a.zip", "b.tar", "file1.txt", "file2.txt", "file10.txt"]
        );

        // 4. Natural Reverse
        worker.set_sort_order(Some(SortOrder::NaturalReverse));
        let (results, _, _, _) = worker.results(
            0,
            10,
            &[100],
            false,
            0,
            Style::default(),
            &mut matcher,
            AutoscrollSettings::default(),
            0,
            (0, false),
            true,
            false,
        );
        let items: Vec<&String> = results.iter().map(|r| r.2).collect();
        assert_eq!(
            items,
            vec!["file10.txt", "file2.txt", "file1.txt", "b.tar", "a.zip"]
        );

        // 5. Extension
        worker.set_sort_order(Some(SortOrder::Extension));
        let (results, _, _, _) = worker.results(
            0,
            10,
            &[100],
            false,
            0,
            Style::default(),
            &mut matcher,
            AutoscrollSettings::default(),
            0,
            (0, false),
            true,
            false,
        );
        let items: Vec<&String> = results.iter().map(|r| r.2).collect();
        // .tar < .txt < .zip
        assert_eq!(
            items,
            vec!["b.tar", "file1.txt", "file2.txt", "file10.txt", "a.zip"]
        );

        // 6. Extension Reverse
        worker.set_sort_order(Some(SortOrder::ExtensionReverse));
        let (results, _, _, _) = worker.results(
            0,
            10,
            &[100],
            false,
            0,
            Style::default(),
            &mut matcher,
            AutoscrollSettings::default(),
            0,
            (0, false),
            true,
            false,
        );
        let items: Vec<&String> = results.iter().map(|r| r.2).collect();
        // .zip > .txt > .tar
        assert_eq!(
            items,
            vec!["a.zip", "file1.txt", "file2.txt", "file10.txt", "b.tar"]
        );

        // 7. None (preserves exact stream/command insertion order)
        worker.set_sort_order(None);
        let (results, _, _, _) = worker.results(
            0,
            10,
            &[100],
            false,
            0,
            Style::default(),
            &mut matcher,
            AutoscrollSettings::default(),
            0,
            (0, false),
            true,
            false,
        );
        let items: Vec<&String> = results.iter().map(|r| r.2).collect();
        assert_eq!(
            items,
            vec!["file10.txt", "file2.txt", "file1.txt", "a.zip", "b.tar"]
        );
    }

    #[test]
    fn test_awt_type_ordering() {
        use crate::Matchmaker;
        use crate::config::*;
        use crate::nucleo::injector::Injector;

        let mut columns = ColumnsConfig::default();
        columns.split = Split::Delimiter(regex::Regex::new("\t").unwrap());
        columns.default = Some(StringValue("type".to_string()));
        columns.names = vec![
            ColumnSetting {
                name: ColumnName("type".to_string()),
                hidden: false,
                ignore: false,
            },
            ColumnSetting {
                name: ColumnName("description".to_string()),
                hidden: false,
                ignore: false,
            },
            ColumnSetting {
                name: ColumnName("icon".to_string()),
                hidden: true,
                ignore: false,
            },
            ColumnSetting {
                name: ColumnName("prefix".to_string()),
                hidden: true,
                ignore: false,
            },
        ];

        let mut worker_config = WorkerConfig::default();
        worker_config.sort_threshold = SortThreshold::SMART;

        let (mut mm, injector, _) = Matchmaker::new_from_config(
            RenderConfig::default(),
            TerminalConfig::default(),
            worker_config,
            columns,
            ExitConfig::default(),
            (false, false),
        );

        let lines = vec![
            "\u{1b}[1;36m feat\u{1b}[0m\tNew feature\t\tfeat/\n",
            "\u{1b}[1;31m fix\u{1b}[0m\tBug fix\t\tfix/\n",
            "\u{1b}[1;33m󰣪 refactor\u{1b}[0m\tCode restructuring\t󰣪\trefactor/\n",
            "\u{1b}[1;35m󰓅 perf\u{1b}[0m\tPerformance optimization\t󰓅\tperf/\n",
            "\u{1b}[1;36m ci\u{1b}[0m\tCI/CD workflows / pipelines\t\tci/\n",
            "\u{1b}[1;34m chore\u{1b}[0m\tMaintenance / config\t\tchore/\n",
            "\u{1b}[1;32m󰧮 docs\u{1b}[0m\tDocumentation\t󰧮\tdocs/\n",
            "\u{1b}[1;36m󰙨 test\u{1b}[0m\tTest suites\t󰙨\ttest/\n",
            "\u{1b}[1;33m󰏖 build\u{1b}[0m\tDependencies / build\t󰏖\tbuild/\n",
            "\u{1b}[2m󰓹 custom\u{1b}[0m\tNo prefix / freeform\t󰓹\t\n",
        ];

        for line in &lines {
            let _ = injector.push((None, line.to_string()));
        }

        mm.worker.nucleo.tick(10);
        let mut matcher = Matcher::default();

        let (results, _, _, _) = mm.worker.results(
            0,
            10,
            &[100, 100, 100, 100],
            false,
            0,
            Style::default(),
            &mut matcher,
            AutoscrollSettings::default(),
            0,
            (0, false),
            true,
            false,
        );

        let items: Vec<String> = results
            .iter()
            .map(|r| {
                // Return column 0 raw or text
                r.1[0].to_string()
            })
            .collect();
        println!("Items returned:\n{:#?}", items);

        assert_eq!(items[0], "\u{1b}[1;36m feat\u{1b}[0m");
        assert_eq!(items[1], "\u{1b}[1;31m fix\u{1b}[0m");
        assert_eq!(items[2], "\u{1b}[1;33m󰣪 refactor\u{1b}[0m");
        assert_eq!(items[3], "\u{1b}[1;35m󰓅 perf\u{1b}[0m");
        assert_eq!(items[4], "\u{1b}[1;36m ci\u{1b}[0m");
        assert_eq!(items[5], "\u{1b}[1;34m chore\u{1b}[0m");
        assert_eq!(items[6], "\u{1b}[1;32m󰧮 docs\u{1b}[0m");
        assert_eq!(items[7], "\u{1b}[1;36m󰙨 test\u{1b}[0m");
        assert_eq!(items[8], "\u{1b}[1;33m󰏖 build\u{1b}[0m");
        assert_eq!(items[9], "\u{1b}[2m󰓹 custom\u{1b}[0m");
    }

    #[test]
    fn test_frecency_mode_preserves_order_and_no_tier_separators() {
        let mut worker = Worker::<String>::new_single_column();
        worker.dir_first = true;
        worker.depth_penalty = 15;
        worker.set_mode_index(1);
        worker.set_sort_order(Some(crate::action::SortOrder::ModifiedReverse));
        worker.set_stability(crate::config::SortThreshold::SMART);

        let paths = vec![
            "/home/fecavmi/Downloads".to_string(),
            "/home/fecavmi/.local/state/omarchy/current/theme/backgrounds".to_string(),
            "/home/fecavmi/.dotfiles/main".to_string(),
            "/home/fecavmi/dev/github/matchmaker/feat-bookmarks".to_string(),
            "/home/fecavmi/.dotfiles/main/.agents".to_string(),
            "/".to_string(),
            "/home/fecavmi".to_string(),
            "/tmp".to_string(),
        ];

        let injector = worker.nucleo.injector();
        for p in &paths {
            injector.push(p.clone(), |item, cols| {
                cols[0] = item.clone().into();
            });
        }

        worker.nucleo.tick(10);

        for (idx, expected) in paths.iter().enumerate() {
            let nth = worker.get_nth(idx as u32);
            assert_eq!(nth, Some(expected), "get_nth mismatch at index {idx}");
        }

        let mut matcher = Matcher::default();
        let (results, _, _, _) = worker.results(
            0,
            paths.len() as u32,
            &[100],
            false,
            0,
            Style::default(),
            &mut matcher,
            AutoscrollSettings::default(),
            0,
            (0, false),
            true,
            false,
        );

        for (idx, (header, _, item_data)) in results.iter().enumerate() {
            eprintln!("{idx}: {item_data} (header={header:?})");
        }
        for (idx, (header, _, item_data)) in results.iter().enumerate() {
            assert_eq!(*header, None, "Expected no tier separator in mode 1");
            assert_eq!(*item_data, &paths[idx], "Mismatch at position {idx}");
        }
    }
}
