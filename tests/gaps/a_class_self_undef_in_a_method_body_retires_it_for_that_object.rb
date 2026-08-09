# `class << self` written inside a METHOD body opens the singleton of the
# INSTANCE the method was called on, not of the class -- so the `undef` here
# retires `close` for one object. logging's `def kill; class << self; undef
# :close; end; end` is exactly this, and it is 98 of the 120 corpus rows the
# `undef` diagnostic covers.
#
# The runtime half is in place: `g.singleton_class.undef_method(:close)` writes
# a per-object tombstone every lookup consults (see
# `tests/a_per_object_undef_retires_the_method_for_that_object.rb`). What is
# missing is the LOWERING -- `class << self` is routed to the compile-time
# `class << self` mapping wherever it is written, so the `undef` becomes a
# `ClassMethodUndef` against the enclosing class, which is a definition-level
# node with no expression form. Codegen refuses it rather than answering
# wrongly.
#
# The fix is a routing one: where `self` is not a class, `class << self` must
# take the same per-object desugar `class << obj` takes.
class Foo
  def close = "class"

  def kill
    class << self
      undef :close
    end
  end
end

g = Foo.new
g.kill
p g.respond_to?(:close)
p(begin
  g.close
rescue NoMethodError
  :raised
end)
p Foo.new.close
