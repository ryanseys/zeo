# `min`/`max`/`last` on a one-sided range are answered from the endpoints, not
# by iterating: an endless range would never finish counting up to its maximum,
# and a beginless one cannot even start.
#
# `min(n)` is the first n counting UP from begin -- which is why an endless
# range can answer it -- and `max(n)` the n largest counting DOWN from end.

# Endless: min counts up and stops, max has no answer at all.
p((1..).min)
p((1..).min(3))
p((5..).min(2))
p((1..).min(0))
p(((1..).max rescue $!.class))
p(((1..).last(2) rescue $!.class))
p(((1..).to_a rescue $!.class))

# Beginless: max counts down from the end, exclusive ends start one lower.
p((..5).max)
p((..5).max(2))
p((..5).max(3))
p((...5).max)
p((...5).max(3))
p((..5).max(0))
p(((..5).min rescue $!.class))

# A Float end has no decrementable element, so a beginless float range keeps
# ruby's answers rather than being counted down.
p((..5.0).max)
p(((...5.0).max rescue $!.class))
p(((..5.0).max(2) rescue $!.class))

# A custom comparator has to walk, so the open side has no answer.
p(((1..).min { |a, b| a <=> b } rescue $!.class))
p(((1..).min(3) { |a, b| a <=> b } rescue $!.class))
p(((..5).max { |a, b| a <=> b } rescue $!.class))

# Bounded ranges are unchanged, including a comparator, a count past the end,
# an exclusive end, a descending range, and non-numeric elements.
p((1..5).min(2))
p((1..5).max(2))
p((1...5).max(2))
p((1..5).min(9))
p((1..5).max(9))
p((1..5).min(2) { |a, b| b <=> a })
p((1..5).max(2) { |a, b| b <=> a })
p((5..1).min(2))
p((5..1).max(2))
p(("a".."e").min(2))
p(("a".."e").max(2))
p((1..5.0).min(2))
p(((1.0..5.0).min(2) rescue $!.class))
p((1..5).last(2))
p((1...5).last(2))

# A negative count is an ArgumentError on either side.
p(((..5).max(-1) rescue $!.message))
p(((1..).min(-1) rescue $!.message))

# A non-Integer count converts.
p((1..).min(3.0))
