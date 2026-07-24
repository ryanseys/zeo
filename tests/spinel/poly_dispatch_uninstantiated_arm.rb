# The poly-dispatch switch drops the `case` arm of a class that is never
# instantiated (no value can carry its cls_id). The critical case: a base class
# that is itself never `.new`'d but whose method is inherited by an instantiated
# subclass must keep that method -- it is referenced through the subclass's arm,
# even though the base's own arm is dropped. Also exercises a same-named method
# on a genuinely-unused class (its arm and method both go away).
class Base
  def kind; "base"; end
end
class Sub < Base            # instantiated; inherits Base#kind
end
class Unused                # never instantiated; same method name
  def kind; "unused"; end
end

items = [Sub.new, Sub.new]  # poly elements; only Sub is instantiated
items.each do |x|
  puts x.kind               # poly dispatch -> Sub arm -> inherited Base#kind
end

# A directly-instantiated class with an overriding method, alongside the base.
class Animal
  def speak; "..."; end
end
class Dog < Animal
  def speak; "woof"; end
end
zoo = [Dog.new, Animal.new]  # both instantiated here
zoo.each { |a| puts a.speak }
