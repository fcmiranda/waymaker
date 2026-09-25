use std::{
    fmt::{self, Debug, Display},
    str::FromStr,
};

use serde::{Deserialize, Serialize, Serializer};

use crate::utils::string::allowed_semantic_char;
use crate::{SSS, utils::serde::StringOrVec};

/// Bindable actions
/// # Additional
/// See [crate::render::render_loop] for the source code definitions.
#[derive(Debug, Clone, PartialEq)]
pub enum Action<A: ActionExt = NullActionExt> {
    /// Add item to selections
    Select,
    /// Remove item from selections
    Deselect,
    /// Remove item from selections and move cursor up
    DeselectUp,
    /// Toggle item in selections
    Toggle,
    /// Toggle item in selections and move cursor up
    ToggleUp,
    /// Toggle all selections
    CycleAll,
    /// Clear all selections
    ClearSelections,
    /// Accept current selection
    Accept,
    /// Quit with code
    Quit(i32),

    // Results
    /// Toggle wrap
    ToggleWrap,
    /// Toggle the action dialog box above the filter input
    ToggleActionBox,
    /// Toggle keyboard focus between input and results when navigation mode is enabled
    ToggleFocus,
    /// Explicitly focus the filter query input in navigation mode
    FocusFilter,
    /// Explicitly focus the results navigation list in navigation mode
    FocusNav,
    /// Toggle parent directory peek 3-pane layout
    ToggleParentPeek,
    /// Toggle footer visibility
    ToggleFooter,
    /// Toggle header visibility
    ToggleHeader,

    // Results Navigation
    /// Move selection index up
    Up(u16),
    /// Move selection index down
    Down(u16),
    Pos(i32),
    // Scroll half page down
    HalfPageDown,
    // Scroll half page up
    HalfPageUp,
    /// Horizontally scroll (the active column of) the current result.
    /// 0 to reset.
    HScroll(i8),
    /// Vertically scroll the current result.
    /// 0 to reset.
    ///
    /// (Rarely useful, unless you have an extremely long result whose wrap overflows.)
    VScroll(i8),

    // Preview
    /// Cycle preview layouts
    CyclePreview,
    /// Show/hide preview for selection
    Preview(String),
    /// Show help in preview
    Help(String),
    /// Set preview layout
    /// None restores the command of the current layout.
    SetPreview(Option<u8>),
    /// Switch preview layout: if the index is already current, the preview is hidden.
    /// None toggles the preview visibility.
    SwitchPreview(Option<u8>),
    /// Toggle wrap in preview
    TogglePreviewWrap,

    // Preview navigation
    /// Scroll preview up
    PreviewUp(u16),
    /// Scroll preview down
    PreviewDown(u16),
    /// Expand preview dimension
    ExpandPreview(u16),
    /// Shrink preview dimension
    ShrinkPreview(u16),
    /// Zoom in preview
    PreviewZoomIn,
    /// Zoom out preview
    PreviewZoomOut,
    /// Scroll preview half page up in rows.
    /// If wrapping is enabled, the visual distance may exceed half a page.
    PreviewHalfPageUp,
    /// Scroll preview half page down in rows.
    /// If wrapping is enabled, the visual distance may exceed half a page.
    PreviewHalfPageDown,

    // experimental
    /// Persistent horizontal scroll
    /// 0 to reset.
    PreviewHScroll(i8),
    /// Persistent single-line vertical scroll
    /// 0 to reset.
    PreviewScroll(i8),
    /// Jump between start, end, initial locations.
    PreviewJump,
    /// Jump to the next Mermaid diagram block in the Markdown preview.
    NextDiagram,
    /// Jump to the previous Mermaid diagram block in the Markdown preview.
    PrevDiagram,
    /// Zoom in diagram preview
    DiagramZoomIn,
    /// Zoom out diagram preview
    DiagramZoomOut,
    /// Reset zoom for diagram preview
    DiagramResetZoom,
    /// Reset zoom for preview
    PreviewResetZoom,
    /// Toggle between document text and diagram image view
    ToggleDiagram,

    /// Cycle columns
    NextColumn,
    /// Cycle columns backwards
    PrevColumn,
    /// Switch to a specific column
    SwitchColumn(String),
    /// Toggle visibility of a column
    ToggleColumn(Option<String>),
    /// Unhide a column, or all columns if None
    ShowColumn(Option<String>),

    // Programmable
    /// Execute command and continue
    Execute(String),
    /// non-blocking [`waymaker::Action::Execute`]: subsequent actions in the batch begin after its completion
    ExecuteAsync(String),
    /// non-blocking [`waymaker::Action::Execute`]: subsequent actions in the batch begin after its completion, only if successful
    ExecuteThen(String),
    /// Execute command without leaving the UI
    ExecuteSilent(String),
    /// Execute command and copy its output to the clipboard
    Copy(String),
    /// Execute command asynchronously and copy its output to the clipboard
    CopyAsync(String),
    /// Exit and become
    Become(String),
    /// Become without exiting the TUI
    BecomeSilent(String),
    /// Reload matcher/worker
    Reload(String),
    /// Change current working directory
    ChDir(String),
    /// Print via handler
    Print(String),
    /// Print key via handler
    PrintKey,
    /// Store a value in the state
    Store(String),

    // Edit (Input)
    /// Move cursor forward char
    ForwardChar,
    /// Move cursor backward char
    BackwardChar,
    /// Move cursor forward word
    ForwardWord,
    /// Move cursor backward word
    BackwardWord,
    /// Delete char
    DeleteChar,
    /// Delete word
    DeleteWord,
    /// Delete next char
    DeleteNextChar,
    /// Delete next word
    DeleteNextWord,
    /// Delete to start of line
    DeleteLineStart,
    /// Delete to end of line
    DeleteLineEnd,
    /// Clear input
    Cancel,
    /// Set input query
    SetQuery(String),
    /// Set query cursor pos
    QueryPos(i32),

    /// Open/toggle the sort options menu in the footer
    SortMenu,
    /// Sort results by specified order, or reset if None
    Sort(Option<SortOrder>),

    // Other/Experimental/Debugging
    /// Insert char into input
    Char(char),
    /// Force redraw
    Redraw,
    /// Custom action
    Custom(A),
    /// Activate the nth overlay
    Overlay(usize),
    /// Alias for a semantic trigger
    Semantic(String),
    /// Set the application mode
    SetMode(String),
    /// A description of a binding, only used for help display.
    Trace(String),
}

/// Result sorting orders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum SortOrder {
    /// Alphabetical (A-Z)
    Alphabetical,
    /// Alphabetical reverse (Z-A)
    AlphabeticalReverse,
    /// Natural sorting (e.g. 1 < 2 < 10)
    #[default]
    Natural,
    /// Natural sorting reverse (10 > 2 > 1)
    NaturalReverse,
    /// Modification time (oldest first)
    Modified,
    /// Modification time reverse (newest first / most recent)
    ModifiedReverse,
    /// Creation / Birth time (oldest first)
    Created,
    /// Creation / Birth time reverse (newest first / most recent)
    CreatedReverse,
    /// File size (smallest first)
    Size,
    /// File size reverse (largest first)
    SizeReverse,
    /// File extension
    Extension,
    /// File extension reverse
    ExtensionReverse,
}

impl Display for SortOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Alphabetical => write!(f, "Alphabetical"),
            Self::AlphabeticalReverse => write!(f, "AlphabeticalReverse"),
            Self::Natural => write!(f, "Natural"),
            Self::NaturalReverse => write!(f, "NaturalReverse"),
            Self::Modified => write!(f, "Modified"),
            Self::ModifiedReverse => write!(f, "ModifiedReverse"),
            Self::Created => write!(f, "Created"),
            Self::CreatedReverse => write!(f, "CreatedReverse"),
            Self::Size => write!(f, "Size"),
            Self::SizeReverse => write!(f, "SizeReverse"),
            Self::Extension => write!(f, "Extension"),
            Self::ExtensionReverse => write!(f, "ExtensionReverse"),
        }
    }
}

impl FromStr for SortOrder {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            "a" | "Alphabetical" | "alphabetical" | "name" | "Name" | "Alpha" | "alpha" => {
                Ok(Self::Alphabetical)
            }
            "A"
            | "AlphabeticalReverse"
            | "alphabetical_reverse"
            | "name_rev"
            | "name_reverse"
            | "name_desc"
            | "AlphaRev"
            | "alpha_rev" => Ok(Self::AlphabeticalReverse),
            "n" | "Natural" | "natural" => Ok(Self::Natural),
            "N" | "NaturalReverse" | "natural_reverse" | "natural_desc" | "NaturalRev"
            | "natural_rev" => Ok(Self::NaturalReverse),
            "m" | "Modified" | "modified" | "mtime" | "Mtime" | "time" => Ok(Self::Modified),
            "M" | "ModifiedReverse" | "modified_reverse" | "mtime_rev" | "mtime_reverse"
            | "mtime_desc" | "MtimeRev" => Ok(Self::ModifiedReverse),
            "b" | "Created" | "created" | "btime" | "Btime" | "birth" | "birth_time"
            | "birthtime" | "ctime" => Ok(Self::Created),
            "B" | "CreatedReverse" | "created_reverse" | "created_rev" | "btime_rev"
            | "btime_reverse" | "btime_desc" | "birth_time_rev" | "BtimeRev" => {
                Ok(Self::CreatedReverse)
            }
            "s" | "Size" | "size" => Ok(Self::Size),
            "S" | "SizeReverse" | "size_reverse" | "size_rev" | "size_desc" | "SizeRev" => {
                Ok(Self::SizeReverse)
            }
            "e" | "Extension" | "extension" | "ext" | "Ext" => Ok(Self::Extension),
            "E" | "ExtensionReverse" | "extension_reverse" | "ext_rev" | "extension_rev"
            | "ExtRev" => Ok(Self::ExtensionReverse),
            other => Err(format!(
                "Unknown sort order: '{other}'. Expected one of: a/A (alphabetical), n/N (natural), m/M (modified), b/B (created/btime), s/S (size), e/E (extension)"
            )),
        }
    }
}

// --------------- MACROS ---------------

/// # Example
/// ```rust
///     use waymaker::{action::{Action, Actions, acs}, render::MMState};
///     pub fn fsaction_aliaser(
///         a: Action,
///         state: &MMState<'_, '_, String, String>,
///     ) -> Actions {
///         match a {
///             Action::Custom(_) => {
///               log::debug!("Ignoring custom action");
///               acs![]
///             }
///             _ => acs![a], // no change
///         }
///     }
/// ```
#[macro_export]
macro_rules! acs {
    ( $( $x:expr ),* $(,)? ) => {
        {
            $crate::action::Actions::from([$($x),*])
        }
    };
}
pub use crate::acs;

/// # Example
/// ```rust
/// #[derive(Debug, Clone, PartialEq)]
/// pub enum FsAction {
///    Filters
/// }
///
/// use waymaker::{binds::{BindMap, bindmap, key}, action::Action};
/// let default_config: BindMap<FsAction> = bindmap!(
///    key!(alt-enter) => Action::Print("".into()),
///    key!(alt-f), key!(ctrl-shift-f) => FsAction::Filters, // custom actions can be specified directly
/// );
/// ```
#[macro_export]
macro_rules! bindmap {
    ( $( $( $k:expr ),+ => $v:expr ),* $(,)? ) => {{
        let mut map = $crate::binds::BindMap::new();
        $(
            let action = $crate::action::Actions::from($v);
            $(
                map.insert($k.into(), action.clone());
            )+
        )*
        map
    }};
} // btw, Can't figure out if its possible to support optional meta over inserts

// --------------- ACTION_EXT ---------------

pub trait ActionExt: Debug + Clone + PartialEq + SSS {}
impl<T: Debug + Clone + PartialEq + SSS> ActionExt for T {}

impl<T> From<T> for Action<T>
where
    T: ActionExt,
{
    fn from(value: T) -> Self {
        Self::Custom(value)
    }
}
#[derive(Debug, Clone, PartialEq)]
pub enum NullActionExt {}

impl fmt::Display for NullActionExt {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        Ok(())
    }
}

impl std::str::FromStr for NullActionExt {
    type Err = ();

    fn from_str(_: &str) -> Result<Self, Self::Err> {
        Err(())
    }
}

// --------------- ACTIONS ---------------
#[derive(Debug, Clone, PartialEq)]
pub struct Actions<A: ActionExt = NullActionExt>(pub Vec<Action<A>>);

impl<A: ActionExt> Default for Actions<A> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<A: ActionExt> From<Vec<Action<A>>> for Actions<A> {
    fn from(v: Vec<Action<A>>) -> Self {
        Actions(v)
    }
}

macro_rules! repeat_impl {
    ($($len:expr),*) => {
        $(
            impl<A: ActionExt> From<[Action<A>; $len]> for Actions<A> {
                fn from(arr: [Action<A>; $len]) -> Self {
                    Actions(Vec::from(arr))
                }
            }

            impl<A: ActionExt> From<[A; $len]> for Actions<A> {
                fn from(arr: [A; $len]) -> Self {
                    Actions(arr.into_iter().map(Action::Custom).collect())
                }
            }
        )*
    }
}
impl<A: ActionExt> From<[Action<A>; 0]> for Actions<A> {
    fn from(empty: [Action<A>; 0]) -> Self {
        Actions(Vec::from(empty))
    }
}
repeat_impl!(1, 2, 3, 4, 5, 6, 7, 8, 9, 10);

impl<A: ActionExt> From<Action<A>> for Actions<A> {
    fn from(action: Action<A>) -> Self {
        acs![action]
    }
}
// no conflict because Action is local type
impl<A: ActionExt> From<A> for Actions<A> {
    fn from(action: A) -> Self {
        acs![Action::Custom(action)]
    }
}

// ---------- SERDE ----------------

impl<A: ActionExt + Display> serde::Serialize for Action<A> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de, A: ActionExt + FromStr> Deserialize<'de> for Actions<A> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let helper = StringOrVec::deserialize(deserializer)?;
        let strings = match helper {
            StringOrVec::String(s) => vec![s],
            StringOrVec::Vec(v) => v,
        };

        let mut actions = Vec::new();
        for s in strings {
            let action = Action::from_str(&s).map_err(serde::de::Error::custom)?;
            actions.push(action);
        }

        Ok(Actions(actions))
    }
}

impl<A: ActionExt + Display> Serialize for Actions<A> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self.0.len() {
            1 => serializer.serialize_str(&self.0[0].to_string()),
            _ => {
                let strings: Vec<String> = self.0.iter().map(|a| a.to_string()).collect();
                strings.serialize(serializer)
            }
        }
    }
}

// ----- action serde
enum_from_str_display!(
    units:
    Select, Deselect, DeselectUp, Toggle, ToggleUp, CycleAll, ClearSelections, Accept,

    HalfPageDown, HalfPageUp,

    ToggleWrap, TogglePreviewWrap, ToggleActionBox, ToggleFocus, FocusFilter, FocusNav, ToggleParentPeek, ToggleFooter, ToggleHeader, CyclePreview, PreviewJump,
    PreviewZoomIn, PreviewZoomOut, PreviewResetZoom,
    NextDiagram, PrevDiagram, DiagramZoomIn, DiagramZoomOut, DiagramResetZoom, ToggleDiagram,

    PreviewHalfPageUp, PreviewHalfPageDown,

    ForwardChar,BackwardChar, ForwardWord, BackwardWord, DeleteChar, DeleteWord, DeleteNextChar, DeleteNextWord, DeleteLineStart, DeleteLineEnd, Cancel, Redraw, NextColumn, PrevColumn, PrintKey, SortMenu;

    tuples:
    Execute, ExecuteAsync, ExecuteThen, ExecuteSilent, Become, BecomeSilent, Preview,
    SetQuery, Pos, QueryPos, SwitchColumn, Store, SetMode,
    CopyAsync, Copy, ChDir;

    defaults:
    (Up, 1), (Down, 1), (PreviewUp, 1), (PreviewDown, 1), (Quit, 130), (Overlay, 0), (Print, String::new()), (Help, String::new()), (Reload, String::new()), (PreviewScroll, 1), (PreviewHScroll, 1), (HScroll, 0), (VScroll, 0), (ExpandPreview, 1), (ShrinkPreview, 1);

    options:
    SwitchPreview, SetPreview, ToggleColumn, ShowColumn, Sort
);

macro_rules! enum_from_str_display {
    (
        units: $( $(#[$uattr:meta])* $unit:ident),*;
        tuples: $( $(#[$tattr:meta])* $tuple:ident),*;
        defaults: $( $(#[$dattr:meta])* ($default:ident, $default_value:expr)),*;
        options: $( $(#[$oattr:meta])* $optional:ident),*
    ) => {
        impl<A: ActionExt + Display> std::fmt::Display for Action<A> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    $(
                        $(#[$uattr])*
                        Self::$unit => write!(f, stringify!($unit)),
                    )*

                    $(
                        $(#[$tattr])*
                        Self::$tuple(inner) => write!(f, concat!(stringify!($tuple), "({})"), inner),
                    )*

                    $(
                        $(#[$dattr])*
                        Self::$default(inner) => {
                            if *inner == $default_value {
                                write!(f, stringify!($default))
                            } else {
                                write!(f, concat!(stringify!($default), "({})"), inner)
                            }
                        },
                    )*

                    $(
                        $(#[$oattr])*
                        Self::$optional(opt) => {
                            if let Some(inner) = opt {
                                write!(f, concat!(stringify!($optional), "({})"), inner)
                            } else {
                                write!(f, stringify!($optional))
                            }
                        },
                    )*

                    Self::Custom(inner) => {
                        write!(f, "{}", inner.to_string())
                    }
                    Self::Char(c) => {
                        write!(f, "{c}")
                    }
                    Self::Semantic(s) => {
                        write!(f, "@{s}")
                    }
                    Self::Trace(s) => {
                        write!(f, "#{s}")
                    }
                }
            }
        }

        impl<A: ActionExt + FromStr> std::str::FromStr for Action<A> {
            type Err = String;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                use crate::utils::string::ALLOWED_CHARS;

                let s = s.trim();
                if let Ok(x) = s.parse::<A>() {
                    return Ok(Self::Custom(x))
                }

                if let Some(s) = s.strip_prefix("@") {
                    if s.chars().all(allowed_semantic_char) && !s.is_empty() {
                        return Ok(Self::Semantic(s.to_string()));
                    } else {
                        return Err(format!("Invalid semantic trigger name: @{s}. Allowed characters are alphanumeric, space, and{}", ALLOWED_CHARS.iter().collect::<String>()));
                    }
                }

                if let Some(s) = s.strip_prefix("#") {
                    return Ok(Self::Trace(s.to_string()));
                }

                let (name, data) = if let Some(pos) = s.find('(') {
                    if s.ends_with(')') {
                        (&s[..pos], Some(&s[pos + 1..s.len() - 1]))
                    } else {
                        (s, None)
                    }
                } else {
                    (s, None)
                };

                match name {
                    $(
                        $(#[$uattr])*
                        n if n.eq_ignore_ascii_case(stringify!($unit)) => {
                            if data.is_some() {
                                Err(format!("Unexpected data for unit variant {}", name))
                            } else {
                                Ok(Self::$unit)
                            }
                        },
                    )*

                    $(
                        $(#[$tattr])*
                        n if n.eq_ignore_ascii_case(stringify!($tuple)) => {
                            let d = data
                            .ok_or_else(|| format!("Missing data for {}", stringify!($tuple)))?
                            .parse()
                            .map_err(|_| format!("Invalid data for {}", stringify!($tuple)))?;
                            Ok(Self::$tuple(d))
                        },
                    )*

                    $(
                        $(#[$dattr])*
                        n if n.eq_ignore_ascii_case(stringify!($default)) => {
                            let d = match data {
                                Some(val) => val
                                .parse()
                                .map_err(|_| format!("Invalid data for {}", stringify!($default)))?,
                                None => $default_value,
                            };
                            Ok(Self::$default(d))
                        },
                    )*

                    $(
                        $(#[$oattr])*
                        n if n.eq_ignore_ascii_case(stringify!($optional)) => {
                            let d = match data {
                                Some(val) if !val.is_empty() => {
                                    Some(
                                        val.parse()
                                        .map_err(|_| format!("Invalid data for {}", stringify!($optional)))?,
                                    )
                                }
                                _ => None,
                            };
                            Ok(Self::$optional(d))
                        },
                    )*

                    _ => Err(format!("Unknown action: {}.", s)),
                }
            }
        }
    };
}
use enum_from_str_display;

impl<A: ActionExt> IntoIterator for Actions<A> {
    type Item = Action<A>;
    type IntoIter = <Vec<Action<A>> as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a, A: ActionExt> IntoIterator for &'a Actions<A> {
    type Item = &'a Action<A>;
    type IntoIter = <&'a Vec<Action<A>> as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<A: ActionExt> FromIterator<Action<A>> for Actions<A> {
    fn from_iter<T: IntoIterator<Item = Action<A>>>(iter: T) -> Self {
        let mut inner = Vec::<Action<A>>::new();
        inner.extend(iter);
        Actions(inner)
    }
}

use std::ops::{Deref, DerefMut};

impl<A: ActionExt> Deref for Actions<A> {
    type Target = Vec<Action<A>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<A: ActionExt> DerefMut for Actions<A> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sort_order_from_str() {
        assert_eq!(SortOrder::from_str("a").unwrap(), SortOrder::Alphabetical);
        assert_eq!(
            SortOrder::from_str("A").unwrap(),
            SortOrder::AlphabeticalReverse
        );
        assert_eq!(SortOrder::from_str("n").unwrap(), SortOrder::Natural);
        assert_eq!(SortOrder::from_str("N").unwrap(), SortOrder::NaturalReverse);
        assert_eq!(SortOrder::from_str("m").unwrap(), SortOrder::Modified);
        assert_eq!(
            SortOrder::from_str("M").unwrap(),
            SortOrder::ModifiedReverse
        );
        assert_eq!(SortOrder::from_str("b").unwrap(), SortOrder::Created);
        assert_eq!(SortOrder::from_str("B").unwrap(), SortOrder::CreatedReverse);
        assert_eq!(SortOrder::from_str("btime").unwrap(), SortOrder::Created);
        assert_eq!(
            SortOrder::from_str("btime_rev").unwrap(),
            SortOrder::CreatedReverse
        );
        assert_eq!(SortOrder::from_str("s").unwrap(), SortOrder::Size);
        assert_eq!(SortOrder::from_str("S").unwrap(), SortOrder::SizeReverse);
        assert_eq!(SortOrder::from_str("e").unwrap(), SortOrder::Extension);
        assert_eq!(
            SortOrder::from_str("E").unwrap(),
            SortOrder::ExtensionReverse
        );
        assert_eq!(SortOrder::from_str("natural").unwrap(), SortOrder::Natural);
        assert_eq!(SortOrder::from_str("mtime").unwrap(), SortOrder::Modified);
        assert_eq!(
            SortOrder::from_str("mtime_rev").unwrap(),
            SortOrder::ModifiedReverse
        );
        assert_eq!(SortOrder::from_str("ext").unwrap(), SortOrder::Extension);
        assert_eq!(
            SortOrder::from_str("ext_rev").unwrap(),
            SortOrder::ExtensionReverse
        );
    }

    #[test]
    fn test_action_sort_from_str_and_display() {
        let a_menu: Action = Action::from_str("SortMenu").unwrap();
        assert_eq!(a_menu, Action::SortMenu);
        assert_eq!(a_menu.to_string(), "SortMenu");

        let a_sort_none: Action = Action::from_str("Sort").unwrap();
        assert_eq!(a_sort_none, Action::Sort(None));
        assert_eq!(a_sort_none.to_string(), "Sort");

        let a_sort_alpha: Action = Action::from_str("Sort(a)").unwrap();
        assert_eq!(a_sort_alpha, Action::Sort(Some(SortOrder::Alphabetical)));
        assert_eq!(a_sort_alpha.to_string(), "Sort(Alphabetical)");

        let a_sort_mtime: Action = Action::from_str("Sort(M)").unwrap();
        assert_eq!(a_sort_mtime, Action::Sort(Some(SortOrder::ModifiedReverse)));
        assert_eq!(a_sort_mtime.to_string(), "Sort(ModifiedReverse)");
    }

    #[test]
    fn test_action_diagram_navigation_and_zoom() {
        let next: Action = Action::from_str("NextDiagram").unwrap();
        assert_eq!(next, Action::NextDiagram);
        assert_eq!(next.to_string(), "NextDiagram");

        let prev: Action = Action::from_str("PrevDiagram").unwrap();
        assert_eq!(prev, Action::PrevDiagram);
        assert_eq!(prev.to_string(), "PrevDiagram");

        let zoom_in: Action = Action::from_str("DiagramZoomIn").unwrap();
        assert_eq!(zoom_in, Action::DiagramZoomIn);
        assert_eq!(zoom_in.to_string(), "DiagramZoomIn");

        let zoom_out: Action = Action::from_str("DiagramZoomOut").unwrap();
        assert_eq!(zoom_out, Action::DiagramZoomOut);
        assert_eq!(zoom_out.to_string(), "DiagramZoomOut");

        let reset_zoom: Action = Action::from_str("DiagramResetZoom").unwrap();
        assert_eq!(reset_zoom, Action::DiagramResetZoom);
        assert_eq!(reset_zoom.to_string(), "DiagramResetZoom");

        let prev_reset: Action = Action::from_str("PreviewResetZoom").unwrap();
        assert_eq!(prev_reset, Action::PreviewResetZoom);
        assert_eq!(prev_reset.to_string(), "PreviewResetZoom");

        let toggle_diag: Action = Action::from_str("ToggleDiagram").unwrap();
        assert_eq!(toggle_diag, Action::ToggleDiagram);
        assert_eq!(toggle_diag.to_string(), "ToggleDiagram");
    }

    #[test]
    fn test_action_units_from_str_and_display() {
        let cases = [
            ("Accept", Action::Accept),
            ("Select", Action::Select),
            ("Deselect", Action::Deselect),
            ("DeselectUp", Action::DeselectUp),
            ("Toggle", Action::Toggle),
            ("ToggleUp", Action::ToggleUp),
            ("CycleAll", Action::CycleAll),
            ("ClearSelections", Action::ClearSelections),
            ("HalfPageDown", Action::HalfPageDown),
            ("HalfPageUp", Action::HalfPageUp),
            ("ToggleWrap", Action::ToggleWrap),
            ("TogglePreviewWrap", Action::TogglePreviewWrap),
            ("ToggleActionBox", Action::ToggleActionBox),
            ("ToggleFocus", Action::ToggleFocus),
            ("FocusFilter", Action::FocusFilter),
            ("FocusNav", Action::FocusNav),
            ("ToggleParentPeek", Action::ToggleParentPeek),
            ("ToggleFooter", Action::ToggleFooter),
            ("ToggleHeader", Action::ToggleHeader),
            ("CyclePreview", Action::CyclePreview),
            ("PreviewJump", Action::PreviewJump),
            ("PreviewHalfPageUp", Action::PreviewHalfPageUp),
            ("PreviewHalfPageDown", Action::PreviewHalfPageDown),
            ("ForwardChar", Action::ForwardChar),
            ("BackwardChar", Action::BackwardChar),
            ("ForwardWord", Action::ForwardWord),
            ("BackwardWord", Action::BackwardWord),
            ("DeleteChar", Action::DeleteChar),
            ("DeleteWord", Action::DeleteWord),
            ("DeleteNextChar", Action::DeleteNextChar),
            ("DeleteNextWord", Action::DeleteNextWord),
            ("DeleteLineStart", Action::DeleteLineStart),
            ("DeleteLineEnd", Action::DeleteLineEnd),
            ("Cancel", Action::Cancel),
            ("Redraw", Action::Redraw),
            ("NextColumn", Action::NextColumn),
            ("PrevColumn", Action::PrevColumn),
            ("PrintKey", Action::PrintKey),
        ];

        for (name, expected) in cases {
            let parsed: Action = Action::from_str(name).unwrap();
            assert_eq!(parsed, expected, "Failed for {name}");
            assert_eq!(parsed.to_string(), name);
        }
    }

    #[test]
    fn test_action_tuples_from_str_and_display() {
        let exec: Action = Action::from_str("Execute(echo hi)").unwrap();
        assert_eq!(exec, Action::Execute("echo hi".into()));
        assert_eq!(exec.to_string(), "Execute(echo hi)");

        let exec_async: Action = Action::from_str("ExecuteAsync(cargo build)").unwrap();
        assert_eq!(exec_async, Action::ExecuteAsync("cargo build".into()));

        let exec_then: Action = Action::from_str("ExecuteThen(cargo test)").unwrap();
        assert_eq!(exec_then, Action::ExecuteThen("cargo test".into()));

        let exec_silent: Action = Action::from_str("ExecuteSilent(touch file)").unwrap();
        assert_eq!(exec_silent, Action::ExecuteSilent("touch file".into()));

        let become_act: Action = Action::from_str("Become(nvim)").unwrap();
        assert_eq!(become_act, Action::Become("nvim".into()));

        let become_silent: Action = Action::from_str("BecomeSilent(zsh)").unwrap();
        assert_eq!(become_silent, Action::BecomeSilent("zsh".into()));

        let preview: Action = Action::from_str("Preview(bat {})").unwrap();
        assert_eq!(preview, Action::Preview("bat {}".into()));

        let set_query: Action = Action::from_str("SetQuery(abc)").unwrap();
        assert_eq!(set_query, Action::SetQuery("abc".into()));

        let pos: Action = Action::from_str("Pos(42)").unwrap();
        assert_eq!(pos, Action::Pos(42));

        let query_pos: Action = Action::from_str("QueryPos(3)").unwrap();
        assert_eq!(query_pos, Action::QueryPos(3));

        let switch_col: Action = Action::from_str("SwitchColumn(col1)").unwrap();
        assert_eq!(switch_col, Action::SwitchColumn("col1".into()));

        let store: Action = Action::from_str("Store(key1)").unwrap();
        assert_eq!(store, Action::Store("key1".into()));

        let set_mode: Action = Action::from_str("SetMode(nav)").unwrap();
        assert_eq!(set_mode, Action::SetMode("nav".into()));

        let copy: Action = Action::from_str("Copy(clip)").unwrap();
        assert_eq!(copy, Action::Copy("clip".into()));

        let copy_async: Action = Action::from_str("CopyAsync(clip)").unwrap();
        assert_eq!(copy_async, Action::CopyAsync("clip".into()));

        let chdir: Action = Action::from_str("ChDir(/tmp)").unwrap();
        assert_eq!(chdir, Action::ChDir("/tmp".into()));
    }

    #[test]
    fn test_action_defaults_and_options() {
        // Defaults
        let up_default: Action = Action::from_str("Up").unwrap();
        assert_eq!(up_default, Action::Up(1));
        assert_eq!(up_default.to_string(), "Up");

        let up_custom: Action = Action::from_str("Up(5)").unwrap();
        assert_eq!(up_custom, Action::Up(5));
        assert_eq!(up_custom.to_string(), "Up(5)");

        let down_default: Action = Action::from_str("Down").unwrap();
        assert_eq!(down_default, Action::Down(1));

        let quit_default: Action = Action::from_str("Quit").unwrap();
        assert_eq!(quit_default, Action::Quit(130));

        let quit_custom: Action = Action::from_str("Quit(0)").unwrap();
        assert_eq!(quit_custom, Action::Quit(0));

        let expand_prev: Action = Action::from_str("ExpandPreview(2)").unwrap();
        assert_eq!(expand_prev, Action::ExpandPreview(2));

        let shrink_prev: Action = Action::from_str("ShrinkPreview(2)").unwrap();
        assert_eq!(shrink_prev, Action::ShrinkPreview(2));

        // Options
        let sw_none: Action = Action::from_str("SwitchPreview").unwrap();
        assert_eq!(sw_none, Action::SwitchPreview(None));
        assert_eq!(sw_none.to_string(), "SwitchPreview");

        let sw_some: Action = Action::from_str("SwitchPreview(1)").unwrap();
        assert_eq!(sw_some, Action::SwitchPreview(Some(1)));
        assert_eq!(sw_some.to_string(), "SwitchPreview(1)");

        let set_prev_none: Action = Action::from_str("SetPreview").unwrap();
        assert_eq!(set_prev_none, Action::SetPreview(None));

        let set_prev_some: Action = Action::from_str("SetPreview(2)").unwrap();
        assert_eq!(set_prev_some, Action::SetPreview(Some(2)));

        let toggle_col_none: Action = Action::from_str("ToggleColumn").unwrap();
        assert_eq!(toggle_col_none, Action::ToggleColumn(None));

        let toggle_col_some: Action = Action::from_str("ToggleColumn(author)").unwrap();
        assert_eq!(toggle_col_some, Action::ToggleColumn(Some("author".into())));

        let show_col_none: Action = Action::from_str("ShowColumn").unwrap();
        assert_eq!(show_col_none, Action::ShowColumn(None));

        let show_col_some: Action = Action::from_str("ShowColumn(author)").unwrap();
        assert_eq!(show_col_some, Action::ShowColumn(Some("author".into())));
    }

    #[test]
    fn test_action_semantics_traces_and_chars() {
        let sem: Action = Action::from_str("@select_item").unwrap();
        assert_eq!(sem, Action::Semantic("select_item".into()));
        assert_eq!(sem.to_string(), "@select_item");

        let trace: Action = Action::from_str("#custom_trace").unwrap();
        assert_eq!(trace, Action::Trace("custom_trace".into()));
        assert_eq!(trace.to_string(), "#custom_trace");

        let ch: Action = Action::Char('j');
        assert_eq!(ch.to_string(), "j");

        assert!(Action::<NullActionExt>::from_str("@invalid*char").is_err());
        assert!(Action::<NullActionExt>::from_str("@").is_err());
        assert!(Action::<NullActionExt>::from_str("NonExistentAction123").is_err());
    }

    #[test]
    fn test_actions_collection() {
        let mut actions: Actions = Actions::from_iter(vec![Action::Up(1), Action::Accept]);
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0], Action::Up(1));

        actions.push(Action::Quit(0));
        assert_eq!(actions.len(), 3);

        let items: Vec<Action> = actions.into_iter().collect();
        assert_eq!(items.len(), 3);

        #[derive(Deserialize, Serialize, PartialEq, Debug)]
        struct TestBinding {
            actions: Actions,
        }

        let parsed_single: TestBinding = toml::from_str(r#"actions = "Up""#).unwrap();
        assert_eq!(parsed_single.actions.len(), 1);
        assert_eq!(parsed_single.actions[0], Action::Up(1));

        let parsed_list: TestBinding = toml::from_str(r#"actions = ["Up", "Accept"]"#).unwrap();
        assert_eq!(parsed_list.actions.len(), 2);
    }
}
