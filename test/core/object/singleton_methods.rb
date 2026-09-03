# Per-object singleton methods (#97): `def obj.foo`, `class << obj`,
# `define_singleton_method`, and the immediate-receiver TypeError.

obj = Object.new
obj.instance_variable_set(:@tag, "A")

def obj.describe
  "object #{@tag}"
end

class << obj
  def shout(word)
    word.upcase
  end
end

obj.define_singleton_method(:twice) { |x| x * 2 }

puts obj.describe
puts obj.shout("hi")
puts obj.twice(21)
puts obj.respond_to?(:describe)
puts obj.respond_to?(:shout)
puts Object.new.respond_to?(:describe)

# A singleton on an immediate is a TypeError.
begin
  5.define_singleton_method(:nope) { 0 }
rescue TypeError => e
  puts "TypeError caught"
end
__END__
object A
HI
42
true
true
false
TypeError caught
