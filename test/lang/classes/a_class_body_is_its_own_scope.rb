# A class or module body is a SEPARATE SCOPE, not a region of the one that
# encloses it. CRuby says so structurally: `NODE_CLASS` compiles the body with
# `NEW_CHILD_ISEQ(..., ISEQ_TYPE_CLASS)` (compile.c), a child iseq with its own
# local table, and `NODE_MODULE` does the same. Only the SUPERCLASS expression
# is compiled into the enclosing iseq -- `COMPILE(ret, "super", nd_super)` sits
# beside the `defineclass` instruction, not inside the child -- which is why
# `class Sub < sup` can read `sup` while `Sub`'s body cannot.
#
# zeo emits each body as its own function for exactly this reason (see
# `codegen::emit_class_body_site_lifted`), so this file is the rule that lift
# stands on. A body that could see an enclosing local would have to be emitted
# inside the enclosing scope, and `fn main` would go back to being the whole
# class-definition phase of the program in one function body.
#
# Every `nil` below is a name the body must NOT see. The last two sections are
# the two ways a body still reaches outward -- a nested body is its own scope
# again, and a body's own locals are ordinary locals an escaping block closes
# over (optparse's accept-table setup is this shape).

outer = :outer

class Scoped
  p defined?(outer)
  inner = :inner
  p inner
  # A `def` is a third scope: it cannot see the class body's locals either.
  def self.reads_inner = defined?(inner)
end
p defined?(inner)
p Scoped.reads_inner

# A REOPEN runs a new body, with a new local table -- the first body's locals
# are gone, not carried forward.
class Scoped
  p defined?(inner)
end

module Moduled
  p defined?(outer)
  m = :m
  p m
end

# The superclass expression runs in the ENCLOSING scope and reads `sup`; the
# body that follows it does not.
sup = Struct.new(:a)
class WithSuper < sup
  p defined?(sup)
end
p WithSuper.new(1).a

class Nesting
  mid = :mid
  class Inner
    p defined?(mid)
  end
  p mid
end

# A body written inside a BLOCK sees no more than one at the top level: the
# block's own `i` is not in scope.
[10].each do |i|
  class InBlock
    p defined?(i)
  end
end

# A body's OWN locals are ordinary locals, and an escaping block closes over
# them the way one written at the top level does.
class Escaping
  seen = []
  %w[a b].each { |el| seen << el }
  p seen
end
__END__
nil
:inner
nil
nil
nil
nil
:m
nil
1
nil
:mid
nil
["a", "b"]
