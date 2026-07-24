# `C.singleton_class.prepend(M)` prepends a module onto a class's SINGLETON
# class, so M's INSTANCE methods become C's CLASS methods at higher priority
# than C's own `def self.x` -- which they override, with `super` reaching the
# original. For a whole-program AOT target this is a static structural fact
# (recorded as `class_method_prepends`, the class-method mirror of `prepends`),
# resolved through the ordinary flattened class-method tables and the singleton
# super chain -- no runtime metaprogramming. Oracle-exact.

# override + super reaching the original class method
module Hook
  def greet
    "[" + super + "]"
  end
end
class Foo
  def self.greet
    "hi"
  end
end
Foo.singleton_class.prepend(Hook)
puts Foo.greet

# super with an argument passthrough
module Audit
  def build(x)
    "audit(" + super + ")"
  end
end
class Maker
  def self.build(x)
    "made:#{x}"
  end
end
Maker.singleton_class.prepend(Audit)
puts Maker.build("wheel")

# multiple prepended modules -> a super chain across both, most-recent closest
module M1
  def tag
    "M1[" + super + "]"
  end
end
module M2
  def tag
    "M2[" + super + "]"
  end
end
class Svc
  def self.tag
    "svc"
  end
end
Svc.singleton_class.prepend(M1)
Svc.singleton_class.prepend(M2)
puts Svc.tag

# full override (the prepended method never calls super)
module Force
  def mode
    "forced"
  end
end
class Engine
  def self.mode
    "stock"
  end
end
Engine.singleton_class.prepend(Force)
puts Engine.mode

# a NEW class method the module adds (no original to override), alongside an
# untouched own class method
module Extra
  def hello
    "hi from extra"
  end
end
class Plain
  def self.name_tag
    "plain"
  end
end
Plain.singleton_class.prepend(Extra)
puts Plain.hello
puts Plain.name_tag
