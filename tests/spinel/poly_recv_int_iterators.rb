# times / upto / downto with a block, on a receiver only known to be an
# Integer at run time. The arm that unboxes a boxed receiver and re-dispatches
# through the typed Integer emitters only ever ran for the BLOCKLESS names, so
# these raised NoMethodError naming Integer, the class that defines them.
def pick(n) = n > 0 ? 3 : "x"

v = pick(1)
acc = []
v.times { |i| acc << i }
v.upto(5) { |i| acc << i }
v.downto(1) { |i| acc << i }
p acc

# each answers the receiver, and a break value is still the call's value
p v.times { |i| i }
p v.upto(5) { |i| i }
p v.downto(1) { |i| i }
p(v.times { |i| break i * 10 if i == 2 })

# a receiver that is not an Integer answers NoMethodError, as CRuby does --
# the plain coercion the blockless names use would have run the loop zero
# times instead
begin
  pick(0).times { |i| p i }
rescue NoMethodError => e
  p e.message
end

# and nil short-circuits through a safe-nav guard
def maybe(n) = n > 0 ? 3 : nil
[1, 0].each do |k|
  w = maybe(k)
  p w&.times { |i| i }
  p w&.upto(5) { |i| i }
  p w&.downto(1) { |i| i }
end
