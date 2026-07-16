# Positional parameter shapes: parenthesized destructuring, the auto-splat
# rule, post-parameter distribution, and the trailing-comma implicit rest.

def one(x) = yield x

# --- Parenthesized destructuring --------------------------------------------
# A single fully-parenthesized param splits the element it receives.
[[1, 2], [3, 4]].each { |(a, b)| p [a, b] }

# Mixed with plain params, at any position.
[[1, [2, 3], 4]].each { |a, (b, c), d| p [a, b, c, d] }
[[[1, 2], 3]].each { |(a, b), c| p [a, b, c] }

# Nested arbitrarily deep.
[[1, [2, [3, 4]]]].each { |a, (b, (c, d))| p [a, b, c, d] }

# A named splat inside the destructure collects the tail; an anonymous one
# consumes its slot and discards it.
[[1, [2, 3, 4]]].each { |a, (b, *r)| p [a, b, r] }
[[1, 2, 3]].each { |a, (*), b| p [a, b] }

# Destructuring works on METHOD params too, not just blocks.
def pair((a, b)) = "#{a}-#{b}"
puts pair([1, 2])

def nested((a, (b, c))) = [a, b, c]
p nested([1, [2, 3]])

# The destructured names are ordinary locals: an escaping block can capture
# them, and reassigning one inside the block is visible outside.
def capture((a, b))
  bump = -> { a += 10 }
  bump.call
  [a, b]
end
p capture([1, 2])

# --- Auto-splat -------------------------------------------------------------
# A lone Array argument spreads across a block's positional params -- but a
# block taking exactly one plain param receives it WHOLE (the `each { |pair| }`
# idiom), and so does a lone optional.
one([1, 2]) { |a| p a }
one([1, 2]) { |a = 9| p a }
one([1, 2]) { |*a| p a }
one([1, 2]) { |a, **k| p a }

# Two or more positional slots spread it.
one([1, 2]) { |a, b| p [a, b] }
one([1, 2]) { |a, *b| p [a, b] }
one([1, 2]) { |*a, b| p [a, b] }

# More than one OPTIONAL spreads it, even with no required param at all --
# while a single optional plus a rest does not.
one([1, 2]) { |a = 5, b = 4| p [a, b] }
one([1, 2]) { |a = 5, *b| p [a, b] }

# A lambda is strict and never auto-splats.
p(->(a) { a }.call([1, 2]))

# --- Post parameters --------------------------------------------------------
# Post params fill left-to-right from whatever the lead/rest left behind, and
# nil-pad when there isn't enough to go around.
one([1, 2]) { |a, *b, c, d| p [a, b, c, d] }
one([1, 2, 3, 4, 5]) { |a, *b, c| p [a, b, c] }
one([1, 2]) { |a, b = 5, c = 6, d, e| p [a, b, c, d, e] }
one([1, 2, 3, 4, 5]) { |a, b = 5, c = 6, d, e| p [a, b, c, d, e] }

# The same clamping in a multi-assignment.
w, *x, y, z = [1, 2]
p [w, x, y, z]

# --- Trailing comma ---------------------------------------------------------
# `|a, |` means "more than one param", which turns on auto-splat, then
# discards everything past `a`.
one([1, 2]) { |a,| p a }

# --- Block-local declarations -----------------------------------------------
# The names after the `;` are FRESH locals scoped to the block: they shadow
# any enclosing local of the same name, are reset to nil on every single
# invocation, and never write back out.
sum = 99
[1].each { |x; sum| sum = x }
p sum                              # 99 -- the outer one is untouched

# Reset per call, so this never accumulates -- `total` is nil again at the
# top of each iteration.
total = 42
[1, 2, 3].each { |x; total| total = (total || 0) + x }
p total                            # 42

# Several at once, and alongside ordinary params.
a = 1
b = 2
[0].each { |z; a, b| a = 7; b = 8 }
p [a, b]                           # [1, 2]

# A nested block's own block-local shadows without disturbing the outer
# block's variable.
r = 0
[5].each do |i|
  [9].each { |j; r| r = j }
  r = i
end
p r                                # 5
