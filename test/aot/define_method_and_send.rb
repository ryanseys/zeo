# Method definition and dispatch at run time: the binary carries the method
# tables and the reflection paths that build them.
class Calculator
  %i[add sub].each do |op|
    define_method(op) { |a, b| op == :add ? a + b : a - b }
  end
end
c = Calculator.new
puts c.send(:add, 2, 3), c.public_send(:sub, 9, 4)
p Calculator.instance_methods(false).sort
p c.method(:add).arity
p c.respond_to?(:mul)
__END__
5
5
[:add, :sub]
2
false
