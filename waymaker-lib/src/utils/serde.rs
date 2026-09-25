use cba::wbog;
use serde::{Deserialize, Deserializer, Serialize};

use crate::utils::string::resolve_escapes;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StringOrVec {
    String(String),
    Vec(Vec<String>),
}
impl Default for StringOrVec {
    fn default() -> Self {
        StringOrVec::String(String::new())
    }
}

pub fn bounded_usize<'de, D, const MIN: usize, const MAX: usize>(d: D) -> Result<usize, D::Error>
where
    D: Deserializer<'de>,
{
    let v = usize::deserialize(d)?;
    if v < MIN {
        wbog!("{} exceeded the the limit of {} and was clamped.", v, MIN);
        Ok(MIN)
    } else if v > MAX {
        wbog!("{} exceeded the the limit of {} and was clamped.", v, MAX);
        Ok(MAX)
    } else {
        Ok(v)
    }
}

pub fn escaped_opt_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    Ok(opt.map(|s| resolve_escapes(&s)))
}

pub fn escaped_opt_char<'de, D>(deserializer: D) -> Result<Option<char>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    match opt {
        Some(s) => {
            let parsed = resolve_escapes(&s);
            let mut chars = parsed.chars();
            let first = chars
                .next()
                .ok_or_else(|| serde::de::Error::custom("escaped string is empty"))?;
            if chars.next().is_some() {
                return Err(serde::de::Error::custom(
                    "escaped string must be exactly one character",
                ));
            }
            Ok(Some(first))
        }
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize, PartialEq)]
    struct TestBounded {
        #[serde(deserialize_with = "bounded_usize::<_, 5, 20>")]
        val: usize,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct TestEscapedString {
        #[serde(default, deserialize_with = "escaped_opt_string")]
        val: Option<String>,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct TestEscapedChar {
        #[serde(default, deserialize_with = "escaped_opt_char")]
        val: Option<char>,
    }

    #[test]
    fn test_bounded_usize_clamping() {
        let low: TestBounded = toml::from_str("val = 2").unwrap();
        assert_eq!(low.val, 5);

        let mid: TestBounded = toml::from_str("val = 12").unwrap();
        assert_eq!(mid.val, 12);

        let high: TestBounded = toml::from_str("val = 50").unwrap();
        assert_eq!(high.val, 20);
    }

    #[test]
    fn test_escaped_opt_string() {
        let s: TestEscapedString = toml::from_str(r#"val = "hello\nworld""#).unwrap();
        assert_eq!(s.val.as_deref(), Some("hello\nworld"));

        let none: TestEscapedString = toml::from_str("").unwrap();
        assert_eq!(none.val, None);
    }

    #[test]
    fn test_escaped_opt_char() {
        let c: TestEscapedChar = toml::from_str(r#"val = "x""#).unwrap();
        assert_eq!(c.val, Some('x'));

        let tab: TestEscapedChar = toml::from_str(r#"val = "\t""#).unwrap();
        assert_eq!(tab.val, Some('\t'));

        let none: TestEscapedChar = toml::from_str("").unwrap();
        assert_eq!(none.val, None);

        let err_empty = toml::from_str::<TestEscapedChar>(r#"val = """#);
        assert!(err_empty.is_err());

        let err_multiple = toml::from_str::<TestEscapedChar>(r#"val = "abc""#);
        assert!(err_multiple.is_err());
    }

    #[test]
    fn test_string_or_vec() {
        #[derive(Debug, Deserialize, PartialEq)]
        struct TestContainer {
            sov: StringOrVec,
        }

        let default_sov = StringOrVec::default();
        assert_eq!(default_sov, StringOrVec::String(String::new()));

        let s: TestContainer = toml::from_str(r#"sov = "single""#).unwrap();
        assert_eq!(s.sov, StringOrVec::String("single".to_string()));

        let v: TestContainer = toml::from_str(r#"sov = ["a", "b", "c"]"#).unwrap();
        assert_eq!(v.sov, StringOrVec::Vec(vec!["a".into(), "b".into(), "c".into()]));
    }
}
