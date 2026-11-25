//! Parameterized rule grammar (#394).
//!
//! A rule descriptor may carry a minimum-amount parameter:
//!
//! ```text
//! rule:<base>:min=<amount>
//! ```
//!
//! where `<base>` is one of the recognized descriptors in
//! `VALID_RULE_DESCRIPTORS` (without the `rule:` prefix) and `<amount>` is a
//! canonical non-negative decimal `i128` (digits only, no sign, no leading
//! zeros except the single digit `0`). The watcher only alerts on events whose
//! amount is at least `<amount>`.

/// Maximum accepted descriptor length in bytes. The longest valid descriptor,
/// `rule:clawback:min=` plus 39 digits, is 57 bytes.
pub const MAX_RULE_LEN: usize = 64;

/// Parses a canonical non-negative decimal `i128`. Returns `None` for empty
/// input, non-digits, leading zeros, or overflow.
pub fn parse_amount(digits: &[u8]) -> Option<i128> {
    if digits.is_empty() || (digits.len() > 1 && digits[0] == b'0') {
        return None;
    }
    let mut v: i128 = 0;
    for &b in digits {
        if !b.is_ascii_digit() {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((b - b'0') as i128)?;
    }
    Some(v)
}

/// If `rule` is `<base>:min=<amount>` for a recognized `base`, returns the
/// parsed minimum amount. Returns `None` for any other shape.
pub fn parse_min_param(rule: &[u8], bases: &[&str]) -> Option<i128> {
    for base in bases {
        let b = base.as_bytes();
        let suffix = b":min=";
        if rule.len() > b.len() + suffix.len()
            && &rule[..b.len()] == b
            && &rule[b.len()..b.len() + suffix.len()] == suffix
        {
            return parse_amount(&rule[b.len() + suffix.len()..]);
        }
    }
    None
}

/// If `rule` is `<base>:min=<amount>` for a recognized `base`, returns that
/// `base` descriptor (#577). Used to treat `rule:<base>` and
/// `rule:<base>:min=N` as the same rule for duplicate checking, regardless of
/// `N`, including `N = 0`.
pub fn base_descriptor<'a>(rule: &[u8], bases: &'a [&'a str]) -> Option<&'a str> {
    for base in bases {
        let b = base.as_bytes();
        let suffix = b":min=";
        if rule.len() > b.len() + suffix.len()
            && &rule[..b.len()] == b
            && &rule[b.len()..b.len() + suffix.len()] == suffix
            && parse_amount(&rule[b.len() + suffix.len()..]).is_some()
        {
            return Some(base);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    const BASES: [&str; 2] = ["rule:transfer", "rule:burn"];

    #[test]
    fn parses_canonical_amounts() {
        assert_eq!(parse_amount(b"0"), Some(0));
        assert_eq!(parse_amount(b"10000"), Some(10_000));
        assert_eq!(
            parse_amount(b"170141183460469231731687303715884105727"),
            Some(i128::MAX)
        );
    }

    #[test]
    fn rejects_malformed_amounts() {
        for s in [&b""[..], b"01", b"-5", b"+5", b"1_0", b"1.5", b" 5", b"0x10",
            b"170141183460469231731687303715884105728"] {
            assert_eq!(parse_amount(s), None);
        }
    }

    #[test]
    fn parses_min_param() {
        assert_eq!(parse_min_param(b"rule:transfer:min=10000", &BASES), Some(10_000));
        assert_eq!(parse_min_param(b"rule:burn:min=0", &BASES), Some(0));
        assert_eq!(parse_min_param(b"rule:mint:min=5", &BASES), None);
        assert_eq!(parse_min_param(b"rule:transfer:min=", &BASES), None);
        assert_eq!(parse_min_param(b"rule:transfer:max=5", &BASES), None);
        assert_eq!(parse_min_param(b"rule:transfer:min=5:min=6", &BASES), None);
    }
}
