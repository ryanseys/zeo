# `Method#parameters` on a BUILTIN row names the parameters ruby names, and
# names nothing where ruby names nothing.
#
# Over 2,594 rows both ruby 4.0.6 and zeo declare, `#parameters` and `#arity`
# agree on every one.
#
# The DSL spells a row's signature per NAME (`params "fmt, buffer: nil"`) in
# ruby's own `def` spelling, and the ARITY falls out of it
# (`zeo_dsl::signature_arity`), so a row that carries a spelling cannot
# disagree with itself. A `ruby def` marks the Rust parameter list AS ruby's
# signature, for the rows where the two lists say the same thing. Both are
# opt-in, because deriving names from every list would invent them for the
# 1,025 rows ruby names nothing on and misreport the 793 whose body shape
# differs on purpose.
#
# The rows CRuby writes in Ruby are annotated by hand rather than carried by
# a vendored Ruby corelib: the Ruby route measures 22.7x slower than the Rust
# rows and 3.5x the bytes, and buys only these signatures.
#
# Five clusters carry the hand-written signatures:
#
#   * Pathname, 44 rows. CRuby writes it in Ruby, so it reports real names.
#     20 rows take a `ruby def` (the Rust parameter names already match);
#     the rest need `params "..."`, because the body's shape genuinely
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
#     bypass the `ruby_class!` DSL, so each carries its own entry to answer
#     ruby's `[[:rest]] / -1`.
#
#   * A generated WRITER. ruby names an `attr_accessor` slot NOTHING and a
#     Struct member's slot `_`. Both are the same generated accessor here, so
#     the attr slot is spelled `__value` (which `param_entries` reports
#     anonymously) and the struct fold renames its slot after lowering --
#     keeping the mark that elides the frame.
#
# The sweeps are `test/gaps/builtin_rows_report_rubys_signature.rb` and
# `test/core/struct/an_accessor_reports_rubys_parameter_name.rb`, plus the per-row Rust
# tables in `builtins/pathname.rs`, `builtins/kernel.rs`,
# `builtins/exception.rs` and `builtins/rstruct.rs`.
# (`Kernel#require` is left out: the oracle runs under `bundler/setup`, whose
# `bundled_gems.rb` replaces it with a Ruby method named `name`.)
p method(:load).parameters
__END__
[[:rest]]
