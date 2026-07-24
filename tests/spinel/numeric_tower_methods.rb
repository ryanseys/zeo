# Integer#bit_length, Integer#fdiv / Float#fdiv, Float#eql? (type-strict),
# and Math.gamma. fdiv always returns a Float; eql? is true only for a Float
# of equal value (no numeric coercion).
p 42.bit_length
p 0.bit_length
p 255.bit_length
p 1024.bit_length

p 10.fdiv(3)
p 7.fdiv(2)
p 2.5.fdiv(2)

p 1.0.eql?(1)
p 1.0.eql?(1.0)
p 2.0.eql?(2)

# eql? is never true for NaN: it returns false when either side is NaN, matching
# CRuby (a NaN matches as a hash key only by object identity, not via eql?). Both
# the float-typed and the boxed-argument paths go through value equality.
def nan; 0.0 / 0.0; end
p nan.eql?(nan)
mixed = [0.0 / 0.0, 7]
p nan.eql?(mixed.first)

p Math.gamma(5)
p Math.gamma(1)

def bl(n)
  n.bit_length
end
p bl(65535)

# Math.gamma raises Math::DomainError at the negative-integer poles, but 0.0
# is +Infinity (not a raising pole).
begin
  Math.gamma(-1.0)
rescue Math::DomainError => e
  puts e.message
end
p Math.gamma(0.0)

# fdiv unboxes a polymorphic argument (here an element of a mixed array).
bases = [2, "x"]
p 10.fdiv(bases.first)
p 2.5.fdiv(bases.first)
