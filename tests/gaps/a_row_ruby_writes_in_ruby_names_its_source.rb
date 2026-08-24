# `#source_location` on a row CRuby writes in RUBY.
#
# CRuby implements 329 method rows across 21 `<internal:>` files -- Ruby it
# compiles into the interpreter -- and each answers a real
# `["<internal:nilclass>", 36]`. zeo implements all of them in Rust, which
# answers `nil`, exactly as ruby answers `nil` for its own C rows.
#
# It is a NARROW divergence: only `#source_location` and the `file:line` tail
# of `Method#inspect` differ. `#parameters`, `#arity`, `#owner`, visibility
# and behaviour all agree row for row, and the whole-surface census is zero
# (`tests/builtin_rows_report_rubys_signature.rb`).
#
# WHY IT IS RUST. zeo vendored CRuby's own `nilclass.rb` and
# `pathname_builtin.rb` and compiled them, which made these rows answer
# byte-identically. It was measured against the Rust rows and dropped
# 2026-08-24 (user-directed):
#
#   * 22.7x SLOWER. 200k iterations of `basename`/`dirname`/`extname`/`+`:
#     0.359 s for the Rust rows against 8.148 s for the compiled Ruby -- and
#     CRuby itself runs the same source in 1.018 s, so zeo's compiled Ruby was
#     8x slower than the interpreter it came from.
#   * 3.5x BIGGER. 88,808 bytes of `__text` for the Rust rows against 314,608
#     for the emitted Ruby bodies, plus a 14.48 MB embedded compiler for the
#     one `eval` in `pathname_builtin.rb`.
#   * It bought only these signatures, and those are now spelled in the DSL.
#
# THE FIX SHAPE, if it is ever worth it. A per-def `at "<internal:nilclass>",
# 36` directive in `ruby_class!`, feeding a builtin source table beside the
# params table that `method_meta::source_location` reads before answering
# `None`. The rows are generated from the oracle: 5 for NilClass, 110 for
# Pathname, 329 across all 21 files. It costs a static table of roughly 16
# bytes a row and no run-time work. It was NOT done here because the same
# measurement says the fidelity is worth less than the table, and because a
# more space-efficient encoding may be worth waiting for.
#
# This file records ruby's answer. zeo answers `nil` on every line.
%i[to_i to_f to_r to_c rationalize].each do |m|
  puts "NilClass##{m}: #{NilClass.instance_method(m).source_location.inspect}"
end
puts "Pathname#basename: #{Pathname.instance_method(:basename).source_location.inspect}"
puts "Method#inspect: #{NilClass.instance_method(:to_i).inspect}"
# The control: a row ruby writes in C answers nil in BOTH, and always has.
puts "String#upcase: #{String.instance_method(:upcase).source_location.inspect}"
