//! `ZEO_RT_TRACE=<topic>[,<topic>...]`: a run-time narration of loading.
//!
//! The compiler has `--log-level`/`ZEO_LOG` and `--dump=<kind>`; the runtime
//! had nothing, so the half of a require that happens while the program runs
//! -- which unit loaded, which autoload fired, which class was revealed --
//! could only be inferred from Ruby-visible side effects. Diagnosing a gem
//! that boots through 500 units by adding a `p` at a time does not scale, and
//! it is how three wrong diagnoses in a row got written into the roadmap.
//!
//! Topics, chosen so each answers one question a load failure raises:
//!
//! | topic | question |
//! |---|---|
//! | `require` | what did this `require` resolve to? |
//! | `unit` | did the unit run, or answer already-loaded? |
//! | `autoload` | was the target registered, and did the read fire it? |
//! | `class` | was the class concealed, and did anything reveal it? |
//!
//! `all` turns on every topic. Output goes to stderr, one line per event,
//! prefixed so it can be grepped out of a program's own output.
//!
//! Deliberately NOT the `tracing` crate: `zeo-rt` links into every compiled
//! program, and a subscriber plus its registry is size and startup the
//! shipped artifact must not pay for. The cost here is one relaxed load of a
//! `u8` per event site, and the sites are all on loading paths that already
//! take a lock.

use std::sync::atomic::{AtomicU8, Ordering};

/// One topic, as a bit in the mask [`enabled`] reads.
#[derive(Clone, Copy)]
pub(crate) enum Topic {
    Require = 0,
    Unit = 1,
    Autoload = 2,
    Class = 3,
}

const NAMES: &[(&str, Topic)] = &[
    ("require", Topic::Require),
    ("unit", Topic::Unit),
    ("autoload", Topic::Autoload),
    ("class", Topic::Class),
];

static MASK: AtomicU8 = AtomicU8::new(UNREAD);
/// The sentinel meaning "the environment has not been parsed yet". A real
/// mask never has the top bit, so this cannot collide with one.
const UNREAD: u8 = 0x80;

fn mask() -> u8 {
    let cached = MASK.load(Ordering::Relaxed);
    if cached != UNREAD {
        return cached;
    }
    let mut bits = 0u8;
    if let Ok(raw) = std::env::var("ZEO_RT_TRACE") {
        for tok in raw.split(',').map(str::trim).filter(|t| !t.is_empty()) {
            if tok == "all" {
                bits = 0x0f;
                continue;
            }
            match NAMES.iter().find(|(n, _)| *n == tok) {
                Some((_, t)) => bits |= 1 << (*t as u8),
                None => eprintln!(
                    "zeo-rt: ZEO_RT_TRACE: unknown topic `{tok}` (known: all, {})",
                    NAMES.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")
                ),
            }
        }
    }
    MASK.store(bits, Ordering::Relaxed);
    bits
}

/// Whether `topic` is on. One relaxed load after the first call.
pub(crate) fn enabled(topic: Topic) -> bool {
    mask() & (1 << (topic as u8)) != 0
}

/// One event. `depth` indents nested loads, so a require chain reads as a
/// tree -- which is the whole point when a gem's boot is 40 files deep.
pub(crate) fn emit(topic: Topic, depth: u32, args: std::fmt::Arguments<'_>) {
    let name = NAMES
        .iter()
        .find(|(_, t)| *t as u8 == topic as u8)
        .map_or("?", |(n, _)| *n);
    let indent = "  ".repeat(depth as usize);
    eprintln!("zeo-rt[{name}]: {indent}{args}");
}

/// `trace!(Topic::Unit, depth, "...", args)` -- the guard and the format are
/// one expression, so a disabled topic costs the load and nothing else.
macro_rules! trace {
    ($topic:expr, $depth:expr, $($arg:tt)*) => {
        if $crate::trace::enabled($topic) {
            $crate::trace::emit($topic, $depth, format_args!($($arg)*));
        }
    };
}
pub(crate) use trace;

/// How a class id reads in a trace line: `Bundler::Source (535)`. An id with
/// no registered name is still printed, because an id the registry does not
/// know is itself the finding.
pub(crate) fn class_label(id: u32) -> String {
    match crate::dispatch::lookup::class_name(crate::ClassId(id)) {
        Some(name) => format!("{name} ({id})"),
        None => format!("<unregistered> ({id})"),
    }
}
