use std::borrow::Cow;

use cba::define_either;
pub use ratatui::text::Text;

define_either! {
    #[derive(serde::Serialize, serde::Deserialize)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
    pub enum Either<L, R = L> {
        Left,
        Right
    }
}

impl Either<Box<str>, Text<'static>> {
    pub fn to_cow(&self) -> Cow<'_, str> {
        match self {
            Either::Left(s) => Cow::Borrowed(s),
            Either::Right(t) => Cow::Owned(t.to_string()),
        }
    }

    pub fn to_text(self) -> Text<'static> {
        match self {
            Either::Left(s) => Text::from(s.into_string()),
            Either::Right(t) => t,
        }
    }

    pub fn as_text(&self) -> Text<'_> {
        match self {
            Either::Left(s) => Text::from(s.as_ref()),
            Either::Right(t) => t.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_either_left_and_right() {
        let left: Either<Box<str>, Text<'static>> = Either::Left("sample left".into());
        assert_eq!(left.to_cow(), Cow::Borrowed("sample left"));
        assert_eq!(left.as_text(), Text::from("sample left"));
        assert_eq!(left.to_text(), Text::from("sample left"));

        let right: Either<Box<str>, Text<'static>> = Either::Right(Text::from("sample right"));
        assert_eq!(right.to_cow(), Cow::Borrowed("sample right"));
        assert_eq!(right.as_text(), Text::from("sample right"));
        assert_eq!(right.to_text(), Text::from("sample right"));
    }
}
