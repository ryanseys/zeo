//! The runtime's own `tracing` output.
//!
//! Two processes run zeo-rt and they get a subscriber by different routes.
//! A `zeo prog.rb` run is the COMPILER's process, and `main.rs` installs one
//! there for `--log-level`/`ZEO_LOG`/`RUST_LOG` -- its filter already names
//! `zeo_rt`, so the events below print with no work here. A COMPILED program
//! is its own process and has no such `main`, which is what [`init`] is for.
//!
//! The subscriber rides the `runtime-logging` feature rather than shipping
//! always. tracing-subscriber with `env-filter` is larger than the whole
//! tolerance the binary-size gate allows (256 KiB against a 7.97 MB floor),
//! and every compiled program would carry it to serve a developer's
//! occasional question. Build the runtime with the feature when a compiled
//! program itself needs to talk.
//!
//! The FACADE is unconditional, so instrumenting the runtime is always
//! allowed: with no subscriber an event is one relaxed load of a global.

/// Install a subscriber for a compiled program, from `ZEO_LOG` or
/// `RUST_LOG`. Idempotent and quiet: a second call, or a process that
/// already has a subscriber (a JIT run, where the compiler installed one),
/// leaves the first in place.
#[cfg(feature = "runtime-logging")]
pub fn init() {
    let Some(directive) = std::env::var("ZEO_LOG")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("RUST_LOG").ok().filter(|s| !s.is_empty()))
    else {
        return;
    };
    // `try_init`, not `init`: the compiler's `main` may already own the
    // global, and losing that race is the correct outcome, not a panic.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(directive))
        .with_writer(std::io::stderr)
        .without_time()
        .try_init();
}

#[cfg(not(feature = "runtime-logging"))]
pub fn init() {}
