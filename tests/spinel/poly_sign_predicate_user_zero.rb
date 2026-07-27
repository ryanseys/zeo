# A user class defining zero? claims the name, so every call on a poly receiver
# goes through the class dispatch. A Float or Integer arriving at that same call
# site still has to answer its own sign from the runtime tag - the dispatch used
# to fall through to the empty default and report false for every number.

class Meter
  def initialize(n) = @n = n
  def zero? = @n == 0
end

def zero(v) = v.zero?
def zero_via(pair) = zero(pair[1])

puts zero_via([:m, Meter.new(5)])
puts zero_via([:m, Meter.new(0)])
puts zero_via([:m, 0.004])
puts zero_via([:m, 0.0])
puts zero_via([:m, 3])
puts zero_via([:m, 0])

# A tag with no sign raises NoMethodError, the same as CRuby.
begin
  puts zero_via([:m, "abc"])
rescue NoMethodError
  puts "NoMethodError"
end
