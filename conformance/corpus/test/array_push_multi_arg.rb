# Array#push / #append take any number of arguments and return the
# array. Both forms previously compiled only the first argument
# (compile_arg0), so a multi-arg push dropped every element after the
# first and the expression-form result of int/float/ptr/poly pushes
# returned 0 instead of the array.

p ["a"].append("b", "c")                    #=> ["a", "b", "c"]
p [1].push(2, 3, 4)                          #=> [1, 2, 3, 4]
p [1.0].push(2.0, 3.0)                       #=> [1.0, 2.0, 3.0]
p [:a].push(:b, :c)                          #=> [:a, :b, :c]
p(["a"].append("b", "c") == ["a", "b", "c"]) #=> true

# Statement form mutates in place.
a = [1]
a.push(2, 3)
p a                                          #=> [1, 2, 3]
a.append(4, 5, 6)
p a                                          #=> [1, 2, 3, 4, 5, 6]

# Single-arg push still returns the array (expression form).
p([1].push(9) == [1, 9])                     #=> true

# Object (ptr) arrays.
class Box
  def initialize(v); @v = v; end
  def v; @v; end
end
b = [Box.new(1)]
b.push(Box.new(2), Box.new(3))
p b.map { |x| x.v }                          #=> [1, 2, 3]

puts "done"
