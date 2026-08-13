# A bare `super` -- no parentheses, no literal block -- forwards the enclosing
# method's own block to the parent, so the method needs its `__blk` parameter
# even though nothing in its body says `yield` or `block_given?`.
#
# This is a stop the block-use walk has to make on the `super` node itself,
# not on anything under it. Collapsing that walk onto `for_each_child` dropped
# the arm, and every method containing a bare `super` silently lost its block
# parameter -- `Gem::Version#freeze` was the first casualty, and the emitted
# Rust simply stopped taking a block.
class Base
  def each_thing
    yield 1
    yield 2
    :base_done
  end
end

class Child < Base
  def each_thing
    super
  end
end

acc = []
p Child.new.each_thing { |v| acc << v }
p acc

# `super()` with an explicit empty argument list forwards the block too --
# only the ARGUMENTS are suppressed, which is a different question.
class Explicit < Base
  def each_thing
    super()
  end
end

acc2 = []
p Explicit.new.each_thing { |v| acc2 << v * 10 }
p acc2

# A LITERAL block on the `super` replaces the forwarded one, so the method
# does not need its own -- but the literal block's body may still use it.
class Literal < Base
  def each_thing
    super { |v| yield(v * 100) }
  end
end

acc3 = []
Literal.new.each_thing { |v| acc3 << v }
p acc3

# A method whose parent never yields still forwards harmlessly.
class Quiet
  def go; :quiet; end
end

class QuietChild < Quiet
  def go; super; end
end

p QuietChild.new.go { :never_called }
