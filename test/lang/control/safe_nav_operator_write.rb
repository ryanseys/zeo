# `o&.v op= rhs` runs nothing at all when the receiver is nil -- not the
# read, not the right-hand side -- and answers nil. The receiver itself is
# evaluated exactly once, before the guard.
class Box
  attr_accessor :v

  def initialize(v) = @v = v
end

def watch(tag, value)
  puts "eval #{tag}"
  value
end

empty = nil
p(empty&.v ||= 8)
p(empty&.v &&= 8)
p(empty&.v += 1)
p(empty&.v = 3)

blank = Box.new(nil)
p(blank&.v ||= 8)
p(blank.v)

counter = Box.new(2)
p(counter&.v += 1)
p(counter&.v &&= 9)
p(counter.v)

p(watch("nil receiver", nil)&.v ||= watch("skipped", 1))
p(watch("real receiver", Box.new(nil))&.v ||= watch("taken", 5))

# `&.` guards nil alone, so `false` still takes the call and raises.
begin
  false&.v ||= 1
rescue NoMethodError => e
  puts e.message
end
__END__
nil
nil
nil
nil
8
8
3
9
9
eval nil receiver
nil
eval real receiver
eval taken
5
undefined method 'v' for false
