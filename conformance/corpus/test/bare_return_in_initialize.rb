# Issue #337: a bare `return` (Ruby's "exit early" form) inside an
# `initialize` body used to lower to `return 0;` regardless of the
# wrapping function's C return type. Two emit shapes were affected:
#
#  - `sp_<C>_new(...)` (sp_<C> *-returning, the heap-allocated
#    constructor synthesis): `return 0;` silently NULLed the
#    instance on the early-return branch — semantically wrong, no C
#    diagnostic.
#
#  - `sp_<C>_initialize(...)` (void-returning, used for `super`
#    chains): `return 0;` is a `void function should not return a
#    value` error under strict C compile.
#
# Fix: pin @current_method_return per emission shape (obj_<C> for
# the _new synthesis, "void" for the _initialize wrapper) so the
# bare-return path picks the right C statement — `return self;` for
# heap constructors (returns the partially-initialized instance,
# matching the issue's expected-behavior table) and `return;` for
# the void wrapper.

class Box
  attr_reader :data
  def initialize(other = nil)
    @data = "ok"
    return if other.nil?
    @data = other
  end
end

# Default-construct: the bare `return` inside initialize fires;
# `@data` stays at "ok".
b1 = Box.new
puts b1.data           # ok

# Pass an explicit value: the early return is skipped; `@data` is
# overwritten.
b2 = Box.new("hi")
puts b2.data           # hi

# Subclass exercising the void `_initialize` super-call form: the
# parent's bare return inside the wrapper must lower to `return;`
# (not `return 0;`, which would be a C error in a void function).
class TaggedBox < Box
  attr_reader :tag
  def initialize(other = nil, tag:)
    super(other)
    @tag = tag
  end
end

t = TaggedBox.new("payload", tag: "label")
puts t.tag             # label

t2 = TaggedBox.new(tag: "empty")
puts t2.tag            # empty
