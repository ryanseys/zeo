class Point
  def initialize(x, y)
    @x = x
    @y = y
  end
end
pt = Point.new(3, 4)
p pt.instance_variables
p pt.instance_variable_set(:@x, 99)
p pt.instance_variable_get(:@x)
p pt.instance_variable_defined?(:@y)
p pt.instance_variable_defined?(:@z)
p 42.instance_variables
p 42.singleton_methods
p 42.respond_to?(:instance_variable_get)
p 42.send(:instance_variables)
p [1, 2, 2, 3].to_set
p (1..3).to_set
p({ a: 1, b: 2 }.to_set.size)
p(/x/.timeout)
p Regexp.timeout
__END__
[:@x, :@y]
99
99
true
false
[]
[]
true
[]
Set[1, 2, 3]
Set[1, 2, 3]
2
nil
nil
