# `Method#owner` answers the CLASS for a class method, where ruby answers that
# class's singleton class. Uniform across user classes and builtins, so it is
# one fix in the class-method branch of `Method#owner` (and the matching
# `UnboundMethod#owner` / `Module#instance_method` on a singleton class): the
# answer should be the receiver's singleton-class id, which
# `runtime_meta::singleton_class_owner` already holds the inverse of.
#
# Noted while writing `tests/builtin_class_method_dispatch.rb`, which leaves
# the assertion out for this reason.
class Foo
  def self.a = 1
end

p Foo.method(:a).owner
p Time.method(:now).owner
p Foo.singleton_class.instance_method(:a).owner
