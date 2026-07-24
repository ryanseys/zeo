# `return a, b` materializes as a fixed-arity tuple struct, preserving
# heterogeneous element types unboxed.

class C
  def initialize
    @x = 1
    @y = 2
    @z = 3
    @s1 = "hi"
    @s2 = "lo"
    @f1 = 1.5
    @f2 = 2.5
  end

  def two_ints
    return @x, @y
  end

  def three_ints
    return @x, @y, @z
  end

  def two_strs
    return @s1, @s2
  end

  def two_floats
    return @f1, @f2
  end

  # Heterogeneous: int + string (no boxing — fields keep concrete types).
  def int_and_str
    return @x, @s1
  end

  # Heterogeneous: int + float + string.
  def three_mixed
    return @x, @f1, @s1
  end

  def consume
    a, b = two_ints
    a + b
  end
end

c = C.new

a, b = c.two_ints
puts a            # 1
puts b            # 2

a, b, d = c.three_ints
puts a            # 1
puts b            # 2
puts d            # 3

s1, s2 = c.two_strs
puts s1           # hi
puts s2           # lo

f1, f2 = c.two_floats
puts f1           # 1.5
puts f2           # 2.5

i, s = c.int_and_str
puts i            # 1
puts s            # hi

i, f, s = c.three_mixed
puts i            # 1
puts f            # 1.5
puts s            # hi

puts c.consume    # 3

puts "done"
