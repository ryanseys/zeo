# CRuby implements 329 method rows across 21 `<internal:>` files -- Ruby it
# compiles into the interpreter -- and each answers a real
# `["<internal:nilclass>", 36]` from `#source_location`. Zeo implements all of
# them in Rust, which answers `nil`, exactly as ruby answers `nil` for its own
# C rows. Only `#source_location` and the `file:line` tail of `Method#inspect`
# differ; `#parameters`, `#arity`, `#owner`, visibility and behaviour agree row
# for row (`test/gaps/builtin_rows_report_rubys_signature.rb`).
#
# The vendored-Ruby alternative is measured and rejected: 22.7x slower, 3.5x
# bigger, plus a 14.48 MB embedded compiler for one `eval` -- it buys only
# these signatures. The fix
# shape, if ever worth it: a per-def `at "<internal:nilclass>", 36` directive
# in `ruby_class!` feeding a builtin source table (~16 bytes/row, no run-time
# work), generated from the oracle.
#
# --- ruby 4.0.6 answers ---
# NilClass#to_i: ["<internal:nilclass>", 36]
# NilClass#to_f: ["<internal:nilclass>", 48]
# NilClass#to_r: ["<internal:nilclass>", 60]
# NilClass#to_c: ["<internal:nilclass>", 24]
# NilClass#rationalize: ["<internal:nilclass>", 12]
# Pathname#basename: ["<internal:pathname_builtin>", 972]
# Method#inspect: #<UnboundMethod: NilClass#to_i() <internal:nilclass>:36>
# String#upcase: nil

# `#source_location` on a row CRuby writes in RUBY (`<internal:>` files).
# Zeo implements those rows in Rust, which answers `nil` -- a decided
# divergence, measured and kept (the vendored-Ruby alternative was 22.7x
# slower and 3.5x bigger). The `.divergence` sidecar records why and the fix
# shape if it is ever worth it; `.expected` records ZEO's answers.
%i[to_i to_f to_r to_c rationalize].each do |m|
  puts "NilClass##{m}: #{NilClass.instance_method(m).source_location.inspect}"
end
puts "Pathname#basename: #{Pathname.instance_method(:basename).source_location.inspect}"
puts "Method#inspect: #{NilClass.instance_method(:to_i).inspect}"
# The control: a row ruby writes in C answers nil in BOTH, and always has.
puts "String#upcase: #{String.instance_method(:upcase).source_location.inspect}"
__END__
NilClass#to_i: nil
NilClass#to_f: nil
NilClass#to_r: nil
NilClass#to_c: nil
NilClass#rationalize: nil
Pathname#basename: nil
Method#inspect: #<UnboundMethod: NilClass#to_i()>
String#upcase: nil
