#![no_main]

use alert_registry::rule_params::{parse_amount, parse_min_param, MAX_RULE_LEN};
use libfuzzer_sys::fuzz_target;

/// Recognized base descriptors (without `rule:` prefix stripped — full form
/// as used by the contract so the fuzz target mirrors real call sites).
const BASES: &[&str] = &[
    "rule:transfer",
    "rule:mint",
    "rule:burn",
    "rule:approve",
    "rule:clawback",
];

fuzz_target!(|data: &[u8]| {
    // Only operate on inputs within the documented maximum length so the
    // fuzzer spends time on the meaningful input space.
    if data.len() > MAX_RULE_LEN {
        return;
    }

    // ── Oracle 1: round-trip ──────────────────────────────────────────────
    // For every base, if `data` happens to be a valid `<base>:min=<n>` rule,
    // format the parsed amount back and verify it round-trips to the same
    // value.
    if let Some(amount) = parse_min_param(data, BASES) {
        // amount must be non-negative (parse_amount only accepts digits).
        assert!(amount >= 0, "parse_min_param returned a negative amount");

        // Re-encode the amount as decimal bytes and re-parse; must be identical.
        // We build the decimal string in a fixed stack buffer (no alloc).
        let mut dec_buf = [0u8; 40]; // i128::MAX is 39 digits
        let dec_len = write_decimal(amount, &mut dec_buf);
        let re_parsed = parse_amount(&dec_buf[..dec_len]);
        assert_eq!(
            re_parsed,
            Some(amount),
            "round-trip failed: {:?} -> {} -> {:?}",
            core::str::from_utf8(data),
            amount,
            re_parsed,
        );
    }

    // ── Oracle 2: parse_amount consistency ───────────────────────────────
    // If the data looks like `<base>:min=<digits>`, the digits suffix
    // processed by parse_amount must agree with parse_min_param's result.
    for base in BASES {
        let b = base.as_bytes();
        let sep = b":min=";
        if data.len() > b.len() + sep.len()
            && &data[..b.len()] == b
            && &data[b.len()..b.len() + sep.len()] == sep
        {
            let digits = &data[b.len() + sep.len()..];
            let direct = parse_amount(digits);
            let via_param = parse_min_param(data, BASES);
            assert_eq!(
                direct, via_param,
                "parse_amount / parse_min_param disagreement on {:?}",
                core::str::from_utf8(data)
            );
        }
    }
});

/// Write `n` as a decimal ASCII string into `buf`.  Returns the number of
/// bytes written.  `buf` must be at least 39 bytes (i128::MAX needs 39 digits).
fn write_decimal(mut n: i128, buf: &mut [u8; 40]) -> usize {
    if n == 0 {
        buf[0] = b'0';
        return 1;
    }
    // Build digits in reverse.
    let mut tmp = [0u8; 40];
    let mut len = 0usize;
    while n > 0 {
        tmp[len] = b'0' + (n % 10) as u8;
        n /= 10;
        len += 1;
    }
    // Reverse into buf.
    for i in 0..len {
        buf[i] = tmp[len - 1 - i];
    }
    len
}
