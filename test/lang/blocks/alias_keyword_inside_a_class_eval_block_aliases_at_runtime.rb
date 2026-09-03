# `alias new old` in a general (non-class-body) context -- here a module's
# `class_eval` block, where `self` is the module -- desugars to a runtime
# `alias_method`, the same path delegate.rb's `kernel.class_eval do alias
# __raise__ raise end` needs.

class Foo
  def greet = "hi"
end
Foo.class_eval do
  alias hello greet
end
puts Foo.new.hello
puts Foo.new.greet
__END__
hi
hi
