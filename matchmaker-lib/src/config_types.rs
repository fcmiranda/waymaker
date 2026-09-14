use std::fmt;

use cba::define_transparent_wrapper;
use ratatui::{
    style::{Color, Modifier, Style},
    widgets::Borders,
};

use regex::Regex;

use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, Visitor},
    ser::SerializeSeq,
};

#[derive(Debug, Default, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
#[matchmaker_partial_macros::partial(
    path,
    derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)
)]
pub struct StyleSetting {
    #[serde(default)]
    // #[serde(deserialize_with = "cba::serde::transform::camelcase_normalized")]
    pub fg: Option<Color>,
    #[serde(default)]
    // #[serde(deserialize_with = "cba::serde::transform::camelcase_normalized")]
    pub bg: Option<Color>,
    pub modifier: Modifier,
}

impl From<StyleSetting> for Style {
    fn from(s: StyleSetting) -> Style {
        Style {
            fg: s.fg,
            bg: s.bg,
            sub_modifier: !s.modifier,
            add_modifier: s.modifier,
            ..Default::default()
        }
    }
}

impl StyleSetting {
    pub fn is_empty(&self) -> bool {
        self.fg.is_none() && self.bg.is_none() && self.modifier.is_empty()
    }

    pub fn r#override(&self, base: Style) -> Style {
        let mut style = base;

        if let Some(fg) = self.fg {
            style = style.fg(fg);
        }

        if let Some(bg) = self.bg {
            style = style.bg(bg);
        }

        style = style.add_modifier(self.modifier);

        style
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum HorizontalSeparator {
    #[default]
    #[serde(alias = "none")]
    None,
    #[serde(alias = "empty")]
    Empty,
    #[serde(alias = "light")]
    Light,
    #[serde(alias = "normal")]
    Normal,
    #[serde(alias = "heavy")]
    Heavy,
    #[serde(alias = "dashed")]
    Dashed,
    #[serde(
        alias = "top",
        alias = "Upper",
        alias = "upper",
        alias = "UpperBlock",
        alias = "upper_block"
    )]
    Top,
    #[serde(
        alias = "bottom",
        alias = "Lower",
        alias = "lower",
        alias = "LowerBlock",
        alias = "lower_block"
    )]
    Bottom,
    #[serde(
        alias = "underline",
        alias = "underlined",
        alias = "under_line",
        alias = "Underline"
    )]
    Underline,
}

impl HorizontalSeparator {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => unreachable!(),
            Self::Empty => " ",
            Self::Light => "─", // U+2500
            Self::Normal => "─",
            Self::Heavy => "━",  // U+2501
            Self::Dashed => "╌", // U+254C (box drawings light double dash)
            Self::Top => "▔",    // U+2594 (upper one eighth block)
            Self::Bottom => " ", // U+2581 (lower one eighth block)
            Self::Underline => "",
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize, Copy)]
pub enum BlinkRate {
    Slow,
    #[default]
    Normal,
    Rapid,
}

impl BlinkRate {
    /// Returns the number of ticks per half-cycle for this blink rate.
    pub fn ticks(self) -> u8 {
        match self {
            BlinkRate::Slow => 90,
            BlinkRate::Normal => 30,
            BlinkRate::Rapid => 10,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum RowConnectionStyle {
    #[default]
    Disjoint,
    Capped,
    Full,
}

define_transparent_wrapper!(
    #[derive(Copy, Clone, serde::Serialize, serde::Deserialize)]
    #[serde(transparent)]
    Count: u16 = 1
);
use ratatui::widgets::Padding as rPadding;

define_transparent_wrapper!(
    #[derive(Copy, Clone, Default)]
    Padding: rPadding
);

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, Copy)]
#[serde(untagged)]
pub enum ShowCondition {
    Bool(bool),
    Free(u16),
}
impl Default for ShowCondition {
    fn default() -> Self {
        Self::Bool(false)
    }
}
impl From<bool> for ShowCondition {
    fn from(value: bool) -> Self {
        ShowCondition::Bool(value)
    }
}

impl From<u16> for ShowCondition {
    fn from(value: u16) -> Self {
        ShowCondition::Free(value)
    }
}

// -----------------------------------------------------------------------------------------

impl Serialize for Padding {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeSeq;
        let padding = self;
        if padding.top == padding.bottom
            && padding.left == padding.right
            && padding.top == padding.left
        {
            serializer.serialize_u16(padding.top)
        } else if padding.top == padding.bottom && padding.left == padding.right {
            let mut seq = serializer.serialize_seq(Some(2))?;
            seq.serialize_element(&padding.left)?;
            seq.serialize_element(&padding.top)?;
            seq.end()
        } else {
            let mut seq = serializer.serialize_seq(Some(4))?;
            seq.serialize_element(&padding.top)?;
            seq.serialize_element(&padding.right)?;
            seq.serialize_element(&padding.bottom)?;
            seq.serialize_element(&padding.left)?;
            seq.end()
        }
    }
}

impl<'de> Deserialize<'de> for Padding {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;
        #[derive(Deserialize)]
        struct PaddingFields {
            #[serde(default)]
            top: u16,
            #[serde(default)]
            right: u16,
            #[serde(default)]
            bottom: u16,
            #[serde(default)]
            left: u16,
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum PaddingFormat {
            Object(PaddingFields),
            Sequence(Vec<u16>),
            Single(u16),
        }

        let format = PaddingFormat::deserialize(deserializer)?;

        let inner = match format {
            // Handle { "top": 10 } or {}
            PaddingFormat::Object(obj) => rPadding {
                top: obj.top,
                right: obj.right,
                bottom: obj.bottom,
                left: obj.left,
            },

            // Handle a raw number: 10
            PaddingFormat::Single(v) => rPadding {
                top: v,
                right: v,
                bottom: v,
                left: v,
            },

            // Handle arrays: [10], [10, 20], or [10, 20, 30, 40]
            PaddingFormat::Sequence(repr) => match repr.len() {
                1 => {
                    let v = repr[0];
                    rPadding {
                        top: v,
                        right: v,
                        bottom: v,
                        left: v,
                    }
                }
                2 => {
                    let lr = repr[0];
                    let tb = repr[1];
                    rPadding {
                        top: tb,
                        right: lr,
                        bottom: tb,
                        left: lr,
                    }
                }
                4 => rPadding {
                    top: repr[0],
                    right: repr[1],
                    bottom: repr[2],
                    left: repr[3],
                },
                _ => {
                    return Err(D::Error::custom(
                        "Expected an object, a number, or an array of 1, 2, or 4 numbers",
                    ));
                }
            },
        };

        Ok(inner.into())
    }
}

// ---------------------------------------------------------------------------------

// define_restricted_wrapper!(
//     #[derive(Clone, serde::Serialize, serde::Deserialize)]
//     #[serde(transparent)]
//     FormatString: String
// );

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Top,
    Bottom,
    Left,
    #[default]
    Right,
}

impl Side {
    pub fn opposite(&self) -> Borders {
        match self {
            Side::Top => Borders::BOTTOM,
            Side::Bottom => Borders::TOP,
            Side::Left => Borders::RIGHT,
            Side::Right => Borders::LEFT,
        }
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CursorSetting {
    None,
    #[default]
    Default,
}

define_transparent_wrapper!(
    #[derive(Clone, Serialize, Default)]
    #[serde(transparent)]
    ColumnName: String
);

impl<'de> Deserialize<'de> for ColumnName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if s.chars().all(|c| c.is_alphanumeric()) {
            Ok(ColumnName(s))
        } else {
            Err(serde::de::Error::custom(format!(
                "Invalid column name '{}': name must be alphanumeric",
                s
            )))
        }
    }
}

// #[matchmaker_partial_macros::partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
#[derive(Default, Debug, Clone, PartialEq, serde::Serialize)]
pub struct ColumnSetting {
    #[serde(default)]
    pub ignore: bool,
    #[serde(default)]
    pub hidden: bool,
    pub name: ColumnName,
}

#[derive(Default, Debug, Clone)]
pub enum Split {
    /// Split by delimiter. Supports regex.
    Delimiter(Regex),
    /// A sequence of regexes.
    Regexes(Vec<Regex>),
    /// No splitting.
    #[default]
    None,
}

impl PartialEq for Split {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Split::Delimiter(r1), Split::Delimiter(r2)) => r1.as_str() == r2.as_str(),
            (Split::Regexes(v1), Split::Regexes(v2)) => {
                if v1.len() != v2.len() {
                    return false;
                }
                v1.iter()
                    .zip(v2.iter())
                    .all(|(r1, r2)| r1.as_str() == r2.as_str())
            }
            (Split::None, Split::None) => true,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------------

impl serde::Serialize for Split {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Split::Delimiter(r) => serializer.serialize_str(r.as_str()),
            Split::Regexes(rs) => {
                let mut seq = serializer.serialize_seq(Some(rs.len()))?;
                for r in rs {
                    seq.serialize_element(r.as_str())?;
                }
                seq.end()
            }
            Split::None => serializer.serialize_none(),
        }
    }
}

impl<'de> Deserialize<'de> for Split {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct SplitVisitor;

        impl<'de> Visitor<'de> for SplitVisitor {
            type Value = Split;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("string for delimiter or array of strings for regexes")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                // Try to compile single regex
                Regex::new(value)
                    .map(Split::Delimiter)
                    .map_err(|e| E::custom(format!("Invalid regex: {}", e)))
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut regexes = Vec::new();
                while let Some(s) = seq.next_element::<String>()? {
                    let r = Regex::new(&s)
                        .map_err(|e| de::Error::custom(format!("Invalid regex: {}", e)))?;
                    regexes.push(r);
                }
                Ok(Split::Regexes(regexes))
            }
        }

        deserializer.deserialize_any(SplitVisitor)
    }
}

impl<'de> Deserialize<'de> for ColumnSetting {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct ColumnStruct {
            #[serde(default)]
            ignore: bool,
            #[serde(default)]
            hidden: bool,
            name: ColumnName,
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Input {
            Str(ColumnName),
            Obj(ColumnStruct),
        }

        match Input::deserialize(deserializer)? {
            Input::Str(name) => Ok(ColumnSetting {
                ignore: true,
                hidden: false,
                name,
            }),
            Input::Obj(obj) => Ok(ColumnSetting {
                ignore: obj.ignore,
                hidden: obj.hidden,
                name: obj.name,
            }),
        }
    }
}

// -------------

#[derive(Debug, Serialize, Clone, PartialEq, Default)]
pub struct EnvValue {
    pub value: String,
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub exec: bool,
}

impl<'de> Deserialize<'de> for EnvValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum RawEnvValue {
            Short(String),
            Long {
                value: String,
                #[serde(default)]
                force: bool,
                #[serde(default)]
                exec: bool,
            },
        }

        match RawEnvValue::deserialize(deserializer)? {
            RawEnvValue::Short(value) => Ok(EnvValue {
                value,
                force: false,
                exec: false,
            }),
            RawEnvValue::Long { value, force, exec } => Ok(EnvValue { value, force, exec }),
        }
    }
}

impl EnvValue {
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            force: false,
            exec: false,
        }
    }
}

// -------------

// #[matchmaker_partial_macros::partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
#[derive(Default, Debug, Clone, PartialEq, serde::Serialize)]
pub struct CommandSetting {
    /// Input separator that only applies when not reading from stdin
    #[serde(default)]
    pub separator: Option<char>,

    #[serde(alias = "cmd")]
    pub command: String,

    #[serde(skip)]
    pub base_command: Option<String>,
}

impl<'de> Deserialize<'de> for CommandSetting {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use crate::utils::serde::escaped_opt_char;

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct CommandStruct {
            /// Input separator that only applies when not reading from stdin
            #[serde(deserialize_with = "escaped_opt_char")]
            #[serde(default)]
            pub separator: Option<char>,

            #[serde(alias = "cmd")]
            pub command: String,
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum CommandSettingDe {
            String(String),
            Full(CommandStruct),
        }

        match CommandSettingDe::deserialize(deserializer)? {
            CommandSettingDe::String(command) => Ok(CommandSetting {
                separator: None,
                command,
                base_command: None,
            }),

            CommandSettingDe::Full(obj) => Ok(CommandSetting {
                separator: obj.separator,
                command: obj.command,
                base_command: None,
            }),
        }
    }
}

// ----------------------------------------------------------------------
pub fn deserialize_string_or_char_as_double_width<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: From<String>,
{
    struct GenericVisitor<T> {
        _marker: std::marker::PhantomData<T>,
    }

    impl<'de, T> Visitor<'de> for GenericVisitor<T>
    where
        T: From<String>,
    {
        type Value = T;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a string or single character")
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            let s = if v.chars().count() == 1 {
                let mut s = String::with_capacity(2);
                s.push(v.chars().next().unwrap());
                s.push(' ');
                s
            } else {
                v.to_string()
            };
            Ok(T::from(s))
        }

        fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            self.visit_str(&v)
        }
    }

    deserializer.deserialize_string(GenericVisitor {
        _marker: std::marker::PhantomData,
    })
}

// ----------------------------------------------------------------------------
define_transparent_wrapper!(
    #[derive(Clone, Eq, Serialize)]
    StringValue: String

);

impl<'de> Deserialize<'de> for StringValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> de::Visitor<'de> for Visitor {
            type Value = StringValue;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a string, number, or bool")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E> {
                Ok(StringValue(v.to_owned()))
            }

            fn visit_string<E>(self, v: String) -> Result<Self::Value, E> {
                Ok(StringValue(v))
            }

            fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E> {
                Ok(StringValue(v.to_string()))
            }

            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E> {
                Ok(StringValue(v.to_string()))
            }

            fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E> {
                Ok(StringValue(v.to_string()))
            }

            fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E> {
                Ok(StringValue(v.to_string()))
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct TestName {
        name: ColumnName,
    }

    #[test]
    fn test_column_name_validation() {
        // Valid
        let name: TestName = toml::from_str("name = \"col1\"").expect("Valid name");
        assert_eq!(name.name.as_str(), "col1");

        let name: TestName = toml::from_str("name = \"Column123\"").expect("Valid name");
        assert_eq!(name.name.as_str(), "Column123");

        // Invalid
        let res: Result<TestName, _> = toml::from_str("name = \"col-1\"");
        assert!(res.is_err());

        let res: Result<TestName, _> = toml::from_str("name = \"col 1\"");
        assert!(res.is_err());

        let res: Result<TestName, _> = toml::from_str("name = \"col_1\"");
        assert!(res.is_err());
    }

    #[derive(Deserialize)]
    struct TestSort {
        sort: SortThreshold,
    }

    #[test]
    fn test_sort_threshold_smart_deserialization() {
        let smart1: TestSort = toml::from_str("sort = \"smart\"").unwrap();
        assert!(smart1.sort.is_smart());

        let smart2: TestSort = toml::from_str("sort = \"auto\"").unwrap();
        assert!(smart2.sort.is_smart());

        let false_threshold: TestSort = toml::from_str("sort = false").unwrap();
        assert_eq!(false_threshold.sort, SortThreshold::NEVER);

        let true_threshold: TestSort = toml::from_str("sort = true").unwrap();
        assert_eq!(true_threshold.sort, SortThreshold::ALWAYS);

        assert_eq!(smart1.sort.get_effective_threshold(""), u32::MAX);
        assert_eq!(smart1.sort.get_effective_threshold("   "), u32::MAX);
        assert_eq!(smart1.sort.get_effective_threshold("gpu"), 0);
    }
}

// ---------------------------------
define_transparent_wrapper!(
    #[derive(Copy, Clone, serde::Serialize)]
    #[serde(transparent)]
    SortThreshold: u32 = 0
);

impl SortThreshold {
    pub const ALWAYS: Self = SortThreshold(0);
    pub const NEVER: Self = SortThreshold(u32::MAX);
    pub const SMART: Self = SortThreshold(u32::MAX - 1);

    pub fn is_smart(&self) -> bool {
        self.0 == u32::MAX - 1
    }

    pub fn get_effective_threshold(&self, query: &str) -> u32 {
        if self.is_smart() {
            if query.trim().is_empty() { u32::MAX } else { 0 }
        } else {
            self.0
        }
    }
}

impl<'de> Deserialize<'de> for SortThreshold {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = SortThreshold;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(
                    f,
                    "a u32, boolean, or string ('smart', 'auto', 'true', 'false')"
                )
            }

            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(SortThreshold(v as u32))
            }

            fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if v < 0 {
                    return Err(E::custom("expected non-negative integer"));
                }

                Ok(SortThreshold(v as u32))
            }

            fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(if v {
                    SortThreshold::ALWAYS
                } else {
                    SortThreshold::NEVER
                })
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                match v.to_lowercase().as_str() {
                    "smart" | "auto" => Ok(SortThreshold::SMART),
                    "true" => Ok(SortThreshold::ALWAYS),
                    "false" => Ok(SortThreshold::NEVER),
                    s => s
                        .parse::<u32>()
                        .map(SortThreshold)
                        .map_err(|_| E::custom(format!("invalid sort option: '{}'", v))),
                }
            }

            fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_str(&v)
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

/// Theme mode for Mermaid diagram rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DiagramTheme {
    /// Detect theme mode automatically from Omarchy system colors.toml or terminal COLORFGBG.
    #[default]
    #[serde(alias = "auto", alias = "Auto", alias = "AUTO", alias = "system", alias = "System")]
    Auto,
    /// Dark theme mode with dark surfaces, vibrant accents, and light text.
    #[serde(alias = "dark", alias = "Dark", alias = "DARK")]
    Dark,
    /// Light theme mode with light surfaces, vibrant accents, and dark text.
    #[serde(alias = "light", alias = "Light", alias = "LIGHT")]
    Light,
}

impl std::str::FromStr for DiagramTheme {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "auto" | "system" => Ok(DiagramTheme::Auto),
            "dark" => Ok(DiagramTheme::Dark),
            "light" => Ok(DiagramTheme::Light),
            other => Err(format!(
                "Invalid diagram theme '{other}', expected 'auto', 'dark', or 'light'"
            )),
        }
    }
}

impl fmt::Display for DiagramTheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiagramTheme::Auto => write!(f, "auto"),
            DiagramTheme::Dark => write!(f, "dark"),
            DiagramTheme::Light => write!(f, "light"),
        }
    }
}

/// Background transparency mode for Mermaid diagram rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DiagramBackground {
    /// Transparent background allowing the terminal or tmux background to show through.
    #[default]
    #[serde(
        alias = "transparent",
        alias = "Transparent",
        alias = "TRANSPARENT",
        alias = "none",
        alias = "clear"
    )]
    Transparent,
    /// Solid background matching the detected or configured theme palette.
    #[serde(
        alias = "solid",
        alias = "Solid",
        alias = "SOLID",
        alias = "opaque"
    )]
    Solid,
}

impl std::str::FromStr for DiagramBackground {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "transparent" | "none" | "clear" => Ok(DiagramBackground::Transparent),
            "solid" | "opaque" => Ok(DiagramBackground::Solid),
            other => Err(format!(
                "Invalid diagram background '{other}', expected 'transparent' or 'solid'"
            )),
        }
    }
}

impl fmt::Display for DiagramBackground {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiagramBackground::Transparent => write!(f, "transparent"),
            DiagramBackground::Solid => write!(f, "solid"),
        }
    }
}
