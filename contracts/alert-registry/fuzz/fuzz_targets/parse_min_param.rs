#![no_main]

use alert_registry::rule_params::{parse_min_param, MAX_RULE_LEN};
use libfuzzer_sys::fuzz_target;

/// All base descriptors recognised by the contract.
const BASES: &[&str] = &[
    "rule:transfer",
    "rule:mint",
    "rule:burn",
    "rule:approve",
    "rule:clawback",
];

fuzz_target!(|data: &[u8]| {
    // Reject inputs that exceed the declared max length — the contract never
    // feeds longer strings into parse_min_param.
    if data.len() > MAX_RULE_LEN {
        return;
    }

    let result = parse_min_param(data, BASES);

    // Round-trip oracle: if parsing succeeded, re-formatting the amount and
    // re-parsing must produce the same value.
    if let Some(amount) = result {
        // Amounts are non-negative i128; format as decimal digits.
        // We use a fixed-size stack buffer to stay no_std / alloc-free.
        let formatted = format_i128(amount);
        let formatted_bytes = formatted.as_bytes();

        // Find which base matched so we can reconstruct the full descriptor.
        for base in BASES {
            let b = base.as_bytes();
            let suffix = b":min=";
            if data.len() > b.len() + suffix.len()
                && &data[..b.len()] == b
                && &data[b.len()..b.len() + suffix.len()] == suffix
            {
                // Build: <base>:min=<formatted_amount>
                let mut reconstructed = [0u8; MAX_RULE_LEN];
                let total = b.len() + suffix.len() + formatted_bytes.len();
                assert!(
                    total <= MAX_RULE_LEN,
                    "reconstructed descriptor exceeds MAX_RULE_LEN"
                );
                reconstructed[..b.len()].copy_from_slice(b);
                reconstructed[b.len()..b.len() + suffix.len()].copy_from_slice(suffix);
                reconstructed[b.len() + suffix.len()..total]
                    .copy_from_slice(formatted_bytes);

                let re_parsed = parse_min_param(&reconstructed[..total], BASES);
                assert_eq!(
                    re_parsed,
                    Some(amount),
                    "round-trip failed: amount={amount}, base={base}"
                );
                break;
            }
        }
    }
});

/// Format a non-negative `i128` as ASCII decimal digits without using
/// `std::fmt` or `alloc` — the fuzz crate is `no_std`-compatible.
///
/// Returns a fixed-size wrapper so we avoid heap allocation.
fn format_i128(mut v: i128) -> DecBuf {
    // i128::MAX is 39 digits; 40 bytes is enough.
    let mut buf = [0u8; 40];
    if v == 0 {
        buf[0] = b'0';
        return DecBuf { buf, len: 1 };
    }
    let mut i = 40usize;
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    // Shift digits to the front of the buffer.
    let len = 40 - i;
    buf.copy_within(i..40, 0);
    DecBuf { buf, len }
}

struct DecBuf {
    buf: [u8; 40],
    len: usize,
}

impl DecBuf {
    fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }
}
