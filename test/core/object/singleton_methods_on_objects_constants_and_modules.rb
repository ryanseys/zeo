# Singleton methods on an ordinary object. Three things that were broken
# together: `super` inside a `class << obj` body (it panicked the compiler with
# "`super` outside a method", since a per-object singleton has no compile-time
# class to splice an ancestor chain against); a block passed to a runtime
# singleton method on a class/module (`M.wrap { }` -- dispatch dropped it and
# `yield` raised LocalJumpError); and `define_singleton_method` on a constant
# that holds an OBJECT rather than naming a class (it was desugared into a
# class reopen, minting a phantom class, so `B.class` answered `Class`).

class Widget
  def initialize(n); @n = n; end
  def label; "w#{@n}"; end
end
W = Widget.new(1)
class << W
  def label; "custom-#{super}"; end
end
p W.label
p W.class

module M; end
def M.wrap; "[" + yield + "]"; end
puts M.wrap { "hi" }

class Box
  def v; 1; end
end
B = Box.new
B.define_singleton_method(:doubled) { v * 2 }
p B.class
p B.doubled

class Named; end
Named.define_singleton_method(:greet) { "hello" }
p Named.greet
__END__
"custom-w1"
Widget
[hi]
Box
2
"hello"
