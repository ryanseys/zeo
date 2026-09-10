# Reopening String and calling the new method on a literal and on a String
# built with new and appended to.
class String
  def shout
    upcase + "!"
  end
end
puts "hello".shout
s = String.new
s << "world"
puts s.shout
puts s
t = String.new("abc")
puts t.shout

# a plain user class still constructs normally
class Point
  def initialize(x); @x = x; end
  def x; @x; end
end
p Point.new(5).x

# a reopened Array still uses the builtin constructor
class Array
  def tag; "arr"; end
end
a = Array.new(3, 0)
a << 9
p a
p [1, 2].tag
__END__
HELLO!
WORLD!
world
ABC!
5
[0, 0, 0, 9]
"arr"
