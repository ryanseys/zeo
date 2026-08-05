# Object#clone copies the receiver's singleton class (dup deliberately does
# not). zeo's clone builds a plain copy of the underlying object, so a
# singleton method defined on the original is gone on the clone -- the copy
# answers the class's method instead.
class Cloneable
  def greet
    "base"
  end
end

o = Cloneable.new
def o.greet
  "singleton " + super
end

puts o.greet
puts o.clone.greet
puts o.dup.greet
puts o.clone.singleton_methods.inspect
puts o.dup.singleton_methods.inspect
