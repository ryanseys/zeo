# `alias` binds to whichever body is ALREADY in effect at the alias
# statement's own position -- here, `Dog`'s own override (found in
# `Dog`'s `own_methods`), not `Animal`'s.

class Animal
  def speak
    "generic"
  end
end
class Dog < Animal
  def speak
    "woof"
  end
  alias original_speak speak
end
d = Dog.new
puts d.speak
puts d.original_speak
__END__
woof
woof
