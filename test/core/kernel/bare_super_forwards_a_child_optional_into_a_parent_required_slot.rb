# The child's and parent's parameter SHAPES differ, so the forwarding
# must flatten positionally rather than match bucket-for-bucket.

class Parent
  def greet(name)
    "Parent(#{name})"
  end
end
class Child < Parent
  def greet(name = "default")
    super
  end
end
puts Child.new.greet
puts Child.new.greet("given")
__END__
Parent(default)
Parent(given)
