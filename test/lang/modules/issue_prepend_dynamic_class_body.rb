# `prepend` called inside a `Class.new { ... }` block raises NoMethodError
# instead of prepending the module -- it only works in a literal `class ...
# end` body.
m = Module.new do
  def foo
    "prepended-" + super
  end
end
klass = Class.new do
  prepend m
  def foo
    "base"
  end
end
p klass.new.foo
__END__
"prepended-base"
