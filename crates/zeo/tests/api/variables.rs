// --- Deeper include/extend/prepend/inherited-ivar coverage,
// oracle-verified against real `ruby` first (added after the initial batch
// above, per the user's request for more comprehensive coverage of these
// specifically).

// Globals, namespaced constants, compound-assignment and
// multi-assignment completeness, call-site splats. Every test below is
// oracle-verified against real `ruby` first, per this project's established
// convention.

// -- Nested classes/modules + constant paths (namespacing).
// Every positive expectation oracle-verified against real ruby 4.0.6.

// ---------------------------------------------------------------------------
// Struct: compile-time class synthesis. Oracle: ruby 4.0.6.
// ---------------------------------------------------------------------------

/// The constant form is the SAME runtime mint as the anonymous form (no
/// compile-time Struct synthesis), so the runtime `Struct.new("Name", :a)`
/// accepts the legacy string-name argument instead of rejecting it at compile
/// time. The runtime names it `Struct::Name`, CRuby's own namespacing --
/// pinned by `tests/five_recorded_divergences_that_are_not.rb`.
#[test]
fn struct_new_string_name_at_const_mints_at_runtime() {
    assert!(zeo::check_program("P = Struct.new(\"Name\", :a)\n").is_ok());
}

// ---------------------------------------------------------------------------
// Builtin method additions (conformance drawdown): Hash[], try_convert,
// Complex.polar, Module#include?, Regexp.last_match, Thread#join/#value, and
// the Random PRNG. Ruby-visible surface; the PRNG cases assert only
// deterministic guarantees (a seeded xorshift64* diverges from CRuby's MT).
// ---------------------------------------------------------------------------
