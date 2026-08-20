# `Super.inherited(C)` fires when the class is CREATED -- once per class, in
# document order, and only for a hook already installed by then.
class Base
  def self.inherited(k)
    puts "inherited #{k}"
    super
  end
end
class A < Base; end
class B < A; end
class A; end  # reopen: creates nothing
class Silent
  def self.inherited(k) = puts("late hook #{k}")
end
class BeforeHook < Silent; end
p [A.name, B.name]
