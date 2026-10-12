//! Input rules from docs/SPEC.md §2.

use crate::Error;
use std::ops::RangeInclusive;

pub const USERNAME_LEN: RangeInclusive<usize> = 3..=20;
pub const PASSWORD_LEN: RangeInclusive<usize> = 8..=128;
pub const POST_MAX_CHARS: usize = 280;
pub const MAX_FOLLOWING: usize = 5_000;
pub const PAGE_DEFAULT: usize = 30;
pub const PAGE_MAX: usize = 100;

pub fn username(s: &str) -> Result<(), Error> {
    if !USERNAME_LEN.contains(&s.len()) || !s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return Err(Error::Invalid("username must be 3-20 characters: letters, digits or _".into()));
    }
    Ok(())
}

pub fn password(s: &str) -> Result<(), Error> {
    if !PASSWORD_LEN.contains(&s.len()) {
        return Err(Error::Invalid("password must be 8-128 bytes".into()));
    }
    Ok(())
}

/// Returns the trimmed body.
pub fn post_body(s: &str) -> Result<String, Error> {
    let body = s.trim();
    let chars = body.chars().count();
    if chars == 0 || chars > POST_MAX_CHARS {
        return Err(Error::Invalid(format!("post must be 1-{POST_MAX_CHARS} characters")));
    }
    if body.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
        return Err(Error::Invalid("post contains control characters".into()));
    }
    Ok(body.to_owned())
}

pub fn page_limit(limit: Option<u32>) -> usize {
    limit.map_or(PAGE_DEFAULT, |l| (l as usize).clamp(1, PAGE_MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usernames() {
        assert!(username("al_1").is_ok());
        assert!(username("ab").is_err());
        assert!(username("a".repeat(21).as_str()).is_err());
        assert!(username("bad name").is_err());
        assert!(username("émile").is_err());
    }

    #[test]
    fn post_bodies() {
        assert_eq!(post_body("  hi\n ").unwrap(), "hi");
        assert!(post_body("   ").is_err());
        assert!(post_body(&"é".repeat(280)).is_ok(), "limit counts chars, not bytes");
        assert!(post_body(&"x".repeat(281)).is_err());
        assert!(post_body("a\u{0007}b").is_err());
    }

    #[test]
    fn page_limits() {
        assert_eq!(page_limit(None), PAGE_DEFAULT);
        assert_eq!(page_limit(Some(0)), 1);
        assert_eq!(page_limit(Some(10_000)), PAGE_MAX);
    }
}
