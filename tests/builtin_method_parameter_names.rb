# `Method#parameters` on a BUILTIN row names the parameters ruby names, and
# names nothing where ruby names nothing.
#
# CLOSED 2026-08-24. The whole-surface census is zero: over 2,594 rows both
# ruby 4.0.6 and zeo declare, `#parameters` and `#arity` agree on every one.
#
# The mechanism landed 2026-08-21 -- the DSL spells a row's signature per NAME
# (`params "fmt, buffer: nil"`) in ruby's own `def` spelling, and the ARITY
# falls out of it (`zeo_dsl::signature_arity`), so a row that carries a
# spelling cannot disagree with itself. A `ruby def` marks the Rust parameter
# list AS ruby's signature, for the rows where the two lists say the same
# thing. Both are opt-in, because deriving names from every list would invent
# them for the 1,025 rows ruby names nothing on and misreport the 793 whose
# body shape differs on purpose.
#
# What was left then was 46 rows -- Pathname (41) and Kernel (5) -- deferred
# to a vendored corelib that would carry CRuby's real signatures. The corelib
# was DROPPED, so they were annotated by hand instead, which the measurement
# says was the right trade anyway: the Ruby route ran 22.7x slower than the
# Rust rows and cost 3.5x the bytes, and bought only these signatures.
#
# Five clusters closed it, and three were not on that list of 46:
#
#   * Pathname, 44 rows. CRuby writes it in Ruby, so it reports real names.
#     20 rows took a `ruby def` (the Rust parameter names already matched);
#     the rest needed `params "..."`, because the body's shape genuinely
#     differs -- `def "read" params "*, **, &" (recv, *rest)`.
#
#   * Kernel, 7 rows. `require` is asymmetric: `Kernel#require` names its
#     parameter and `Kernel.require` does not, because CRuby defines the two
#     separately -- so it is two defs here rather than one `module_function`.
#
#   * `Dir#initialize`, `Hash#initialize`, `Time#initialize`, whose C
#     functions declare real signatures.
#
#   * Every exception class that declares its own `initialize`. All twelve
#     bypass the `ruby_class!` DSL, so they reached no table at all and
#     answered `[] / 0` where ruby says `[[:rest]] / -1`.
#
#   * A generated WRITER. ruby names an `attr_accessor` slot NOTHING and a
#     Struct member's slot `_`; zeo spelled both `value`. Both are the same
#     generated accessor here, so the attr slot became `__value` (which
#     `param_entries` reports anonymously) and the struct fold renames its
#     slot after lowering -- keeping the mark that elides the frame.
#
# The sweeps are `tests/builtin_rows_report_rubys_signature.rb` and
# `tests/an_accessor_reports_rubys_parameter_name.rb`, plus the per-row Rust
# tables in `builtins/pathname.rs`, `builtins/kernel.rs`,
# `builtins/exception.rs` and `builtins/rstruct.rs`.
p method(:require).parameters
