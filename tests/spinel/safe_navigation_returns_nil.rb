# Issue #701: safe navigation `&.` on nil returns nil.

# Literal nil receiver — short-circuits without warning.
v1 = nil&.foo
puts v1.inspect

# Dynamic nullable receiver — short-circuits when nil, dispatches when not.
def maybe_str(b)
  b ? "hello" : nil
end

puts maybe_str(true)&.upcase
puts maybe_str(false)&.upcase.inspect

# Non-nil case — `&.` behaves identical to `.`.
s = "world"
puts s&.length

# Hash recv (typed nullable from RBS / return inference).
def maybe_hash(b)
  return nil unless b
  h = {}
  h["k"] = "v"
  h
end

puts maybe_hash(true)&.length
puts maybe_hash(false)&.length.inspect
