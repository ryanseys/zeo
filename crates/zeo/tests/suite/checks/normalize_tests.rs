//! The scrubbing every corpus comparison passes through.
//! `tests/corpus/normalize.rs` holds the functions; the corpus binary runs
//! under datatest, which has no libtest harness to run these.

use crate::normalize::{normalize_addresses, normalize_thread_ids};

fn scrub(s: &str) -> String {
    String::from_utf8(normalize_addresses(s.as_bytes().to_vec())).unwrap()
}

#[test]
fn pointer_width_hex_runs_are_scrubbed() {
    assert_eq!(
        scrub("#<Thread:0x0000000102cf6310 t.rb:4 run>"),
        "#<Thread:0xADDR t.rb:4 run>"
    );
    assert_eq!(scrub("a 0xdeadbeefcafef00d b"), "a 0xADDR b");
    // A Rust abort backtrace prints ASLR'd frame addresses without the
    // leading zeros -- 8..=16 digits all scrub (the typed-diff leg
    // compares two zeo RUNS, where these differ per process).
    assert_eq!(scrub("19: 0x1066f30d0 - zeo_rt_x"), "19: 0xADDR - zeo_rt_x");
    // `%x`/`%a` formatting is shorter, and a longer run isn't an address.
    assert_eq!(scrub("0xff / 010"), "0xff / 010");
    assert_eq!(scrub("0x1234567 short"), "0x1234567 short");
    assert_eq!(scrub("\"0x1.ffp+7\""), "\"0x1.ffp+7\"");
    assert_eq!(scrub("0x00000001234567890"), "0x00000001234567890");
    // Uppercase hex is `%X` output, never an address rendering.
    assert_eq!(scrub("0xDEADBEEFCAFEF00D"), "0xDEADBEEFCAFEF00D");
}

#[test]
fn panic_thread_ids_are_scrubbed() {
    let n = |s: &str| String::from_utf8(normalize_thread_ids(s.as_bytes().to_vec())).unwrap();
    assert_eq!(
        n("thread 'ruby-main' (156051069) panicked"),
        "thread 'ruby-main' (TID) panicked"
    );
    // A parenthesized number NOT after a quote stays.
    assert_eq!(n("count (42) stays"), "count (42) stays");
}
