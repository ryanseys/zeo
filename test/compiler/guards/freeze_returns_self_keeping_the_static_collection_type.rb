# Exercises `types.rs`'s `.freeze`-returns-self inference: `names[0]`/
# `names.length` must still take the static Array fast path (a Poly
# fallback would panic). A LOCAL, not the classic `NAMES = [...].freeze`
# constant idiom: a constant READ is always Poly (constants live in a
# runtime map with no static type tracking) -- a PRE-existing gap that
# makes `A = [1]; A[0]` fail with or without `.freeze` involved, noted
# for a later phase, not a freeze regression.

names = ["a", "b"].freeze
puts names[0]
puts names.length
puts names.frozen?
__END__
a
2
true
