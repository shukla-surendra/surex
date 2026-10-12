//! Input rules from docs/SPEC.md §2.

use crate::Error;
use std::ops::RangeInclusive;

pub const USERNAME_LEN: RangeInclusive<usize> = 3..=20;
pub const PASSWORD_LEN: RangeInclusive<usize> = 8..=128;
pub const POST_MAX_CHARS: usize = 280;
pub const MAX_FOLLOWING: usize = 5_000;
pub const PAGE_DEFAULT: usize = 30;
pub const PAGE_MAX: usize = 100;
pub const DISPLAY_NAME_MAX_CHARS: usize = 50;
pub const BIO_MAX_CHARS: usize = 160;
pub const MAX_MEDIA: usize = 4;
pub const MEDIA_MAX_BYTES: usize = 5 * 1024 * 1024;
/// Longest side, in pixels, of an accepted image.
pub const MEDIA_MAX_SIDE: u32 = 8_000;
pub const ALT_MAX_CHARS: usize = 420;

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
    let body = post_body_optional(s)?;
    if body.is_empty() {
        return Err(Error::Invalid(format!("post must be 1-{POST_MAX_CHARS} characters")));
    }
    Ok(body)
}

/// Like `post_body`, but an empty body is allowed (posts that carry images).
pub fn post_body_optional(s: &str) -> Result<String, Error> {
    let body = s.trim();
    let chars = body.chars().count();
    if chars > POST_MAX_CHARS {
        return Err(Error::Invalid(format!("post must be at most {POST_MAX_CHARS} characters")));
    }
    if body.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
        return Err(Error::Invalid("post contains control characters".into()));
    }
    Ok(body.to_owned())
}

/// Returns the trimmed display name; empty clears it.
pub fn display_name(s: &str) -> Result<String, Error> {
    let name = s.trim();
    if name.chars().count() > DISPLAY_NAME_MAX_CHARS || name.chars().any(char::is_control) {
        return Err(Error::Invalid(format!("display name must be at most {DISPLAY_NAME_MAX_CHARS} characters on one line")));
    }
    Ok(name.to_owned())
}

/// Returns the trimmed bio; empty clears it.
pub fn bio(s: &str) -> Result<String, Error> {
    let bio = s.trim();
    if bio.chars().count() > BIO_MAX_CHARS {
        return Err(Error::Invalid(format!("bio must be at most {BIO_MAX_CHARS} characters")));
    }
    if bio.chars().any(|c| c.is_control() && c != '\n') {
        return Err(Error::Invalid("bio contains control characters".into()));
    }
    Ok(bio.to_owned())
}

/// Returns the trimmed alt text; empty is allowed.
pub fn alt_text(s: &str) -> Result<String, Error> {
    let alt = s.trim();
    if alt.chars().count() > ALT_MAX_CHARS || alt.chars().any(char::is_control) {
        return Err(Error::Invalid(format!("alt text must be at most {ALT_MAX_CHARS} characters on one line")));
    }
    Ok(alt.to_owned())
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
    fn profile_fields() {
        assert_eq!(display_name("  Ada Lovelace ").unwrap(), "Ada Lovelace");
        assert_eq!(display_name("").unwrap(), "");
        assert!(display_name(&"x".repeat(51)).is_err());
        assert!(display_name("two\nlines").is_err());
        assert!(bio("line one\nline two").is_ok());
        assert!(bio(&"é".repeat(161)).is_err());
    }

    #[test]
    fn page_limits() {
        assert_eq!(page_limit(None), PAGE_DEFAULT);
        assert_eq!(page_limit(Some(0)), 1);
        assert_eq!(page_limit(Some(10_000)), PAGE_MAX);
    }
}
