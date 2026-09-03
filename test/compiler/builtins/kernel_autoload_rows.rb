# `Kernel#autoload`/`#autoload?` -- the receiverless spellings, which ruby
# files as private instance methods AND public singleton methods of `Kernel`.
# zeo answered `Kernel.autoload` off `Module`, so the call worked but every
# reflection question named the wrong owner.

p Kernel.singleton_methods(false).grep(/autoload/).sort
p Kernel.private_instance_methods(false).grep(/autoload/).sort
p Kernel.instance_methods(false).grep(/autoload/)
p [Kernel.method(:autoload).arity, Kernel.method(:autoload?).arity]

# The registration lands on `Object`, not on `Kernel` -- which is what makes a
# top-level `autoload` visible to every class.
p Kernel.autoload?(:Sprocket)
Kernel.autoload(:Sprocket, "sprocket")
p Kernel.autoload?(:Sprocket)
p Object.autoload?(:Sprocket)
p Module.new.autoload?(:Sprocket)

# Private on every object, so a receiverless send reaches it and an explicit
# one does not.
p Object.new.respond_to?(:autoload, true)
p Object.new.respond_to?(:autoload)
p Object.new.send(:autoload?, :Sprocket)

# A constant that already resolved reports nil, whatever was registered.
Kernel.autoload(:Integer, "integer")
p Kernel.autoload?(:Integer)
__END__
[:autoload, :autoload?]
[:autoload, :autoload?]
[]
[2, -1]
nil
"sprocket"
"sprocket"
nil
true
false
"sprocket"
nil
