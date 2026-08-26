# An `undef_method` written inside a `class << self` body is invisible to
# `respond_to?`, though the CALL correctly refuses. The same undef written
# `class << Foo` is seen by both.
#
# `class << self` retags a `def` onto the class-method channel
# (`SINGLETON_BODY_DEF`), and `undef_method` is not a `def` -- so the undef
# lands as an INSTANCE undef on the surrogate module. Dispatch finds it
# there, which is why the call raises; `class_method_undefined`, which
# `respond_to?` asks, does not.
#
# Found while giving `BigDecimal` back ruby's own missing `new` -- the json
# gem's `decimal_class:` protocol branches on exactly this predicate.
class Foo
  class << self
    undef_method :new
  end
end
p Foo.respond_to?(:new)
begin
  Foo.new
rescue NoMethodError => e
  p e.class
end

class Bar; end
class << Bar
  undef_method :new
end
p Bar.respond_to?(:new)
