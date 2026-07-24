# Endless methods (Ruby 3.0+): `def name(args) = expr` is shorthand for
# a single-expression method body. Prism flattens this to the same
# DefNode shape as a regular def with a one-statement StatementsNode
# body, so the existing codegen path supports it transparently. This
# test pins that contract — if a future Prism upgrade emits a distinct
# node type (e.g. SingleLineMethodDefinitionNode), the test breaks
# loudly instead of silently regressing.

# 1. Plain integer return.
def double(x) = x * 2
puts double(21)
# 42

# 2. String return with interpolation.
def greet(name) = "hello, #{name}"
puts greet("world")
# hello, world

# 3. Self-recursive endless method (still expressible, since the body
#    is one expression).
def fact(n) = n <= 1 ? 1 : n * fact(n - 1)
puts fact(5)
# 120

# 4. No-arg endless method.
def answer = 42
puts answer
# 42

# 5. Endless method using a block-passing call.
def doubled_max(arr) = arr.map { |x| x * 2 }.max
puts doubled_max([1, 4, 2])
# 8

# 6. Class endless method (instance + class methods).
class Box
  def self.unit = 1
  def initialize(v); @v = v; end
  def double = @v * 2
  def name = "Box"
end

puts Box.unit
# 1
puts Box.new(7).double
# 14
puts Box.new(0).name
# Box

# 7. Boolean-returning endless method.
def positive?(n) = n > 0
puts positive?(10)
# true
puts positive?(-1)
# false

# 8. Method composition (one endless method calling another).
def square(x) = x * x
def square_of_sum(a, b) = square(a + b)
puts square_of_sum(2, 3)
# 25
