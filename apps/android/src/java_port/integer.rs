//! `java.lang.Integer` parsing.

/// `Integer.parseInt(s)` (ASCII digits, optional sign, no overflow).
pub fn parse_int(s: &str) -> Option<i32> {
    let (negative, digits) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut value: i64 = 0;
    for b in digits.bytes() {
        value = value * 10 + (b - b'0') as i64;
        if value > i32::MAX as i64 + 1 {
            return None;
        }
    }
    let value = if negative { -value } else { value };
    i32::try_from(value).ok()
}

#[cfg(test)]
mod tests {
    use super::parse_int;

    #[test]
    fn parse_int_matches_java() {
        assert_eq!(parse_int("+80"), Some(80));
        assert_eq!(parse_int("-5"), Some(-5));
        assert_eq!(parse_int("2147483647"), Some(i32::MAX));
        assert_eq!(parse_int("-2147483648"), Some(i32::MIN));
        assert_eq!(parse_int("2147483648"), None);
        assert_eq!(parse_int(""), None);
        assert_eq!(parse_int("-"), None);
        assert_eq!(parse_int("1 "), None);
    }
}
