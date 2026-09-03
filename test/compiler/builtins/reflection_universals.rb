# Universal reflection, to_set, and Regexp#timeout.

class Point
  def initialize(x, y)
    @x = x
    @y = y
  end
end

pt = Point.new(3, 4)
p pt.instance_variables
p pt.instance_variable_get(:@x)
p pt.instance_variable_set(:@x, 99)
p pt.instance_variable_get(:@x)
p pt.instance_variable_defined?(:@y)
p pt.instance_variable_defined?(:@z)

# Value types expose no ivars and no per-object singleton methods.
p 42.instance_variables
p 42.instance_variable_get(:@a)
p "s".singleton_methods

# respond_to? now agrees with the universal reflection methods.
p 42.respond_to?(:instance_variables)
p 42.respond_to?(:instance_variable_get)
p 42.respond_to?(:singleton_methods)

# A dynamic send resolves them too.
p 42.send(:instance_variables)

# to_set reaches Array, Range, and Hash through Enumerable.
p [1, 2, 2, 3].to_set
p (1..3).to_set
p({ a: 1, b: 2 }.to_set.size)

# Regexp timeout reports the (absent) default.
p(/x/.timeout)
p Regexp.timeout
__END__
[:@x, :@y]
3
99
99
true
false
[]
nil
[]
true
true
true
[]
Set[1, 2, 3]
Set[1, 2, 3]
2
nil
nil
