# `is_a?`, `kind_of?` and `instance_of?` on a receiver the compiler cannot
# give one type. Every kind has to answer for itself: Integer, String, Float,
# nil, true/false, Symbol, and the built-in Array, Hash and Range, as well as
# a user class.
#
# The shape is a mixed array whose elements come back out one at a time and
# route to a branch by type. A subclass instance must select its own arm and
# NOT its sibling's, so the walk goes from the object towards its ancestors.

class Foo
  def name; "foo"; end
end
class Bar < Foo
  def name; "bar"; end
end

arr = [42, "hello", :sym, 1.5, nil, true, false, [1, 2], Foo.new, Bar.new]
arr.each do |x|
  if x.is_a?(Integer)
    puts "Integer #{x}"
  elsif x.is_a?(String)
    puts "String #{x}"
  elsif x.is_a?(Symbol)
    puts "Symbol #{x.to_s}"
  elsif x.is_a?(Float)
    puts "Float"
  elsif x.is_a?(NilClass)
    puts "Nil"
  elsif x.is_a?(TrueClass)
    puts "True"
  elsif x.is_a?(FalseClass)
    puts "False"
  elsif x.is_a?(Array)
    puts "Array"
  elsif x.is_a?(Bar)
    puts "Bar"
  elsif x.is_a?(Foo)
    puts "Foo"
  else
    puts "?"
  end
end
__END__
Integer 42
String hello
Symbol sym
Float
Nil
True
False
Array
Foo
Bar
