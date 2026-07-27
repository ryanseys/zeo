# zero?/positive?/negative? on a POLY receiver must test the number itself, not
# its integer truncation. A parameter widened to poly by one call site reading an
# element off a heterogeneous array answered as though every value with |v| < 1
# were zero: 0.004.zero? came back true and 0.004.positive? false. That poisoned
# EVERY call site of the widened method, including ones passing a float literal.

def sign(v) = "#{v.zero?} #{v.positive?} #{v.negative?}"

# Widens `sign`'s parameter to poly: [Symbol, Float] has a union element type.
def sign_via(pair) = sign(pair[1])

puts sign(0.004) # the monomorphic call site is poisoned too
puts sign_via([:t, 0.004])
puts sign_via([:t, -0.004])
puts sign_via([:t, 0.0])
puts sign_via([:t, 2.5])
puts sign_via([:t, 7])
puts sign_via([:t, -7])
puts sign_via([:t, 0])

# A bigint reports its own sign rather than a wrapped int64's.
puts sign_via([:t, 10**30])
puts sign_via([:t, -(10**30)])

# A Rational answers from its numerator, exactly: a ratio too small to survive
# a double would otherwise round to zero and lose its sign.
puts sign_via([:t, Rational(1, 2)])
puts sign_via([:t, Rational(0, 5)])
puts sign_via([:t, Rational(-3, 4)])

# even?/odd? stay integer predicates - a Float does not answer them in Ruby - so
# truncation cannot lose a value that legally reaches them.
def parity(n) = "#{n.even?} #{n.odd?}"
def parity_via(pair) = parity(pair[1])

puts parity_via([:n, 4])
puts parity_via([:n, 7])
