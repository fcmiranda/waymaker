use std::fmt;

use cba::define_restricted_wrapper;
use serde::{Deserialize, Deserializer};

define_restricted_wrapper!(
    #[derive(Clone, Copy, serde::Serialize, PartialOrd, Eq, Ord)]
    #[serde(transparent)]
    Percentage: u16 = 100
);
impl Percentage {
    pub fn new(value: u16) -> Self {
        if value <= 100 { Self(value) } else { Self(100) }
    }

    /// Rounds up
    pub fn compute_clamped(&self, total: u16, min: u16, max: u16) -> u16 {
        if total == 0 {
            return 0;
        }
        let effective_max = if max == 0 { total } else { max };
        let effective_min = min.min(effective_max);
        let pct_height = (total * self.inner()).div_ceil(100);
        pct_height.clamp(effective_min, effective_max)
    }

    pub fn complement(&self) -> Self {
        Self(100 - self.0)
    }

    pub fn saturating_sub(&self, other: u16) -> Self {
        Self(self.0.saturating_sub(other))
    }
}

impl fmt::Display for Percentage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}%", self.0)
    }
}

impl TryFrom<u16> for Percentage {
    type Error = String;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        if value > 100 {
            Err(format!("Percentage out of range: {}", value))
        } else {
            Ok(Self::new(value))
        }
    }
}
impl<'de> Deserialize<'de> for Percentage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let v = u16::deserialize(deserializer)?;
        v.try_into().map_err(serde::de::Error::custom)
    }
}
impl std::str::FromStr for Percentage {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim_end_matches('%');
        let v: u16 = s
            .parse()
            .map_err(|e: std::num::ParseIntError| format!("Invalid number: {}", e))?;
        v.try_into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_clamped_zero_total() {
        let p = Percentage::new(50);
        // total == 0 with min = 1, max = 0 previously panicked with min > max
        assert_eq!(p.compute_clamped(0, 1, 0), 0);
        assert_eq!(p.compute_clamped(0, 0, 0), 0);
        assert_eq!(p.compute_clamped(0, 5, 10), 0);
    }

    #[test]
    fn test_compute_clamped_normal() {
        let p = Percentage::new(50);
        assert_eq!(p.compute_clamped(100, 10, 80), 50);
        assert_eq!(p.compute_clamped(10, 1, 0), 5);
        assert_eq!(p.compute_clamped(100, 60, 80), 60);
        assert_eq!(p.compute_clamped(100, 10, 40), 40);
    }

    #[test]
    fn test_compute_clamped_min_greater_than_max() {
        let p = Percentage::new(50);
        // min > max should not panic
        assert_eq!(p.compute_clamped(100, 80, 40), 40);
    }
}
