# It prepends the module there as it does in a literal class body, so super
# reaches the original.
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
