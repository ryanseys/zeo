# It undefines the method there as it does in a literal class body, which is
# what a bare `require "delegate"` needs.
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
__END__
1
false
