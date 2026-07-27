# Kernel#pp on a Hash generates Rust that fails to type-check: it wraps an
# already-Mutex-wrapped value (`Arc<Mutex<RawMutex, RubyValue>>`) in another
# `parking_lot::Mutex::new(value)`, which expects a plain `RubyValue`. zeo
# fails to compile this program instead of producing a runnable binary.
require "pp"
pp({a: 1})
