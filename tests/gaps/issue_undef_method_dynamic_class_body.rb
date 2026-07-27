# undef_method called inside a `Class.new { ... }` / `#class_eval { ... }`
# block raises NoMethodError instead of undefining the method -- it only
# works in a literal `class ... end` body. This breaks bare `require
# "delegate"`: Delegator's setup calls `kernel.class_eval { undef_method m }`
# on a duplicated Kernel module.
klass = Class.new do
  def foo
    1
  end

  def bar
    2
  end
end
klass.class_eval do
  undef_method :bar
end
o = klass.new
p o.foo
p o.respond_to?(:bar)
