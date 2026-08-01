# A hash pattern that misses a KEY raises `NoMatchingPatternKeyError` in CRuby,
# carrying the key it wanted and the Hash it asked of. zeo raises the parent
# `NoMatchingPatternError` with no detail.
#
# The accessors themselves are implemented and match CRuby when the exception
# is constructed directly (`tests/encoding_error_rows.rb`); what is missing is
# the RAISE SITE. `codegen/patterns.rs` compiles a whole pattern to one boolean,
# so by the time the `if !(#cond)` arm runs, nothing knows which key failed.
# Fix shape: have `emit_hash_pattern` write the failing key into a local the
# raise arm reads, and emit the subclass with `matchee:`/`key:` when that local
# is set.

begin
  { a: 1 } => { b: }
rescue NoMatchingPatternError => e
  puts "class: #{e.class}"
  puts "key: #{e.respond_to?(:key) ? e.key.inspect : 'no accessor'}"
end
