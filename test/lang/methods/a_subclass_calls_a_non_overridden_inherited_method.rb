# Closes a real, pre-existing latent gap: a class's own LITERAL methods
# each get a generated Rust method, but `Dog.new.speak` with no override
# at all has none to call (Rust has no cross-struct inherent-method
# inheritance). `analyze::mro`'s
# materialization fixes this as a byproduct of doing modules correctly
# (every MRO-reachable method gets its own Scope on the receiver).

class Animal
  def speak
    "generic"
  end
end
class Dog < Animal
end
puts Dog.new.speak
__END__
generic
