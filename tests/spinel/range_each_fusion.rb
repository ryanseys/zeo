# Range#each over a literal numeric range should fuse to a tight C
# for-loop with bounds inlined. Output is identical to the unfused
# path; this test pins behavioural parity. The perf win shows up in
# the generated C — no sp_Range struct copy on the loop's hot path.

# --- Basic bounds ---

# Inclusive range, accumulator
sum = 0
(1..10).each { |i| sum = sum + i }
puts sum

# Exclusive range — last value is excluded
sum_excl = 0
(1...10).each { |i| sum_excl = sum_excl + i }
puts sum_excl

# Negative bounds
neg_sum = 0
(-3..3).each { |i| neg_sum = neg_sum + i }
puts neg_sum

# Crosses zero, exclusive
zero_sum = 0
(-2...2).each { |i| zero_sum = zero_sum + i }
puts zero_sum

# --- Edge sizes ---

# Single-element inclusive range
hits = 0
(7..7).each { |i| hits = hits + 1 }
puts hits

# Empty exclusive range (start == end, exclusive)
empties = 0
(5...5).each { |i| empties = empties + 1 }
puts empties

# Zero-element inclusive range at the origin
zero_hits = 0
(0..0).each { |i| zero_hits = zero_hits + 1 }
puts zero_hits

# Decreasing range — start > end. CRuby yields zero times; the
# fused for-loop's `i <= hi` check is false on entry.
dec_count = 0
(10..1).each { |i| dec_count = dec_count + 1 }
puts dec_count

# Decreasing exclusive range — same zero-iteration shape.
dec_excl = 0
(10...1).each { |i| dec_excl = dec_excl + 1 }
puts dec_excl

# --- Block-param scoping ---

# Block param doesn't leak when its name is distinct from outer
# locals. (Same-name shadowing is a pre-existing spinel-wide
# limitation in compile_each_block — both this fusion path and the
# generic sp_Range path emit the block param as `lv_<name>`, which
# collides with an outer `lv_<name>` of the same name. Out of scope
# for this perf PR; see PR body.)
outer_v = 42
(1..5).each { |inner| outer_v = outer_v + inner }
puts outer_v

# --- Non-trivial bounds ---

# Non-literal bounds (variable-bounded range) — falls back to the
# generic sp_Range path. Behaviour must still match.
lo = 2
hi = 6
var_sum = 0
(lo..hi).each { |x| var_sum = var_sum + x }
puts var_sum

# Parens-wrapped range — `((1..5))` is a ParenthesesNode wrapping a
# RangeNode. The fusion recognizer must unwrap parens before
# deciding whether to inline.
p_sum = 0
((1..5)).each { |i| p_sum = p_sum + i }
puts p_sum

# --- Control flow inside the loop ---

# `break` exits the loop early; the accumulator captures only the
# values seen before the break.
early = 0
(1..100).each { |i| break if i > 5; early = early + i }
puts early

# `next` skips the body for the current iteration; even values
# contribute, odd values are skipped.
even_sum = 0
(1..10).each { |i| next if i % 2 != 0; even_sum = even_sum + i }
puts even_sum

# --- Nesting ---

# Nested ranges
total = 0
(1..3).each { |i| (1..3).each { |j| total = total + i * j } }
puts total

# Triple-nested with mixed inclusive/exclusive
mixed = 0
(1..2).each { |a| (1...3).each { |b| (0..1).each { |c| mixed = mixed + a * b + c } } }
puts mixed
