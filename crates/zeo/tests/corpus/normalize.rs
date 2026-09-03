//! Scrubbing that makes a golden comparable across processes: object
//! addresses and panic thread ids differ per run, so both sides pass
//! through here before the diff.

/// Scrub object identity, which is process-random on both sides: CRuby and
/// zeo both render it as `0x` followed by exactly 16 lowercase hex digits
/// (`#<Thread:0x0000000102cf6310 ...>`, `#<Object:0x...>`), so a golden can
/// assert the shape AROUND an address it could never match.
///
/// Deliberately exactly 16: `%x`-formatted output in the corpus (`0xff`,
/// `0x1.ffp+7`) is far shorter and stays untouched. Programs that would
/// rather scrub Ruby-side (`e.message.sub(/0x[0-9a-f]+/, "0xADDR")`, the
/// existing convention) keep working -- their output has no address left in
/// it by the time it gets here.
pub fn normalize_addresses(bytes: Vec<u8>) -> Vec<u8> {
    // 8..=16 hex digits: a 16-digit run is ruby's own `#<Object:0x...>`
    // rendering; 9-12 digit runs are the ASLR'd frame addresses a Rust
    // abort backtrace prints (the typed-diff leg compares two zeo RUNS,
    // where those differ per process). Anything shorter stays -- a
    // program legitimately printing `0x1f` keeps its value.
    const MIN: usize = 8;
    const MAX: usize = 16;
    let is_hex = |b: u8| b.is_ascii_digit() || (b'a'..=b'f').contains(&b);
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"0x") {
            let run = bytes[i + 2..].iter().take_while(|&&b| is_hex(b)).count();
            if (MIN..=MAX).contains(&run) {
                out.extend_from_slice(b"0xADDR");
                i += 2 + run;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// `thread 'ruby-main' (156051069) panicked` -> the id becomes `TID`.
/// Rust's panic header embeds the OS thread id, which differs per process
/// -- the typed-diff leg compares two zeo RUNS, and a committed golden
/// that embedded one would be flaky already, so scrubbing is pure gain.
pub fn normalize_thread_ids(bytes: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let tid = bytes[i..].starts_with(b"' (")
            && bytes[i + 3..]
                .iter()
                .take_while(|b| b.is_ascii_digit())
                .count()
                > 0
            && {
                let n = bytes[i + 3..]
                    .iter()
                    .take_while(|b| b.is_ascii_digit())
                    .count();
                bytes.get(i + 3 + n) == Some(&b')')
            };
        if tid {
            let n = bytes[i + 3..]
                .iter()
                .take_while(|b| b.is_ascii_digit())
                .count();
            out.extend_from_slice(b"' (TID)");
            i += 3 + n + 1;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}
