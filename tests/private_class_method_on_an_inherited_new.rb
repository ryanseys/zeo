# `private_class_method :new` marks a name the class does not DEFINE -- it
# inherits `Class#new` -- so every path that asked "does this class have a class
# method by this name" answered no and let the public `Class#new` through.
# singleton.rb writes exactly this, from its `included` hook, which is why no
# class body carries the mark and codegen cannot see it.
require "singleton"

class Only
  include Singleton
end

p Only.instance.equal?(Only.instance)
begin
  Only.new
rescue NoMethodError => e
  p [e.class, e.message]
end
p Only.respond_to?(:new)
p Only.respond_to?(:new, true)
p Only.singleton_class.private_method_defined?(:new)
p Only.singleton_methods(false)

# The same mark written directly, both spellings.
class Direct
  private_class_method :new
end
begin; Direct.new; rescue NoMethodError => e; p e.class; end
p Direct.respond_to?(:new)

class Runtime2; end
Runtime2.private_class_method :new, :allocate
begin; Runtime2.new; rescue NoMethodError => e; p e.class; end
begin; Runtime2.allocate; rescue NoMethodError => e; p e.class; end
p Runtime2.respond_to?(:new)
p Runtime2.singleton_class.private_method_defined?(:new)

# `public_class_method` puts it back.
Runtime2.public_class_method :new
p Runtime2.respond_to?(:new)
p Runtime2.new.class

# An ordinary class is untouched.
class Plain3; end
p Plain3.respond_to?(:new)
p Plain3.singleton_class.private_method_defined?(:new)
p Plain3.new.class

# A private `new` is still reachable through an implicit self inside the class.
class Factory
  private_class_method :new
  def self.build = new
end
p Factory.build.class
