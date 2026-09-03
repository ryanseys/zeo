# `super`'s three written shapes mean three different things, and a bare
# `super` forwards the current method's arguments AS CURRENTLY BOUND --
# including a parameter that is optional here but required in the parent.

class Parent
  def greet(name)
    "Parent(#{name})"
  end
end

# Bare `super` forwards the child's optional parameter into the parent's
# REQUIRED slot -- the shapes differ, so a bucket-by-bucket match wouldn't
# bind the parent's `name` at all.
class BareSuper < Parent
  def greet(name = "default")
    super
  end
end
puts BareSuper.new.greet
puts BareSuper.new.greet("given")

# `super()` with empty parens passes NO arguments -- the opposite meaning.
class ParenSuper < Parent
  def greet(name = "child")
    super("explicit")
  end
end
puts ParenSuper.new.greet

# Bare `super` forwards the CURRENT value, reassignments included.
class Reassigned < Parent
  def greet(name)
    name = name.upcase
    super
  end
end
puts Reassigned.new.greet("ada")

# Flattening works across buckets: required + optional + post.
class Multi
  def show(a, b, c, d)
    "a=#{a} b=#{b} c=#{c} d=#{d}"
  end
end
class MultiChild < Multi
  def show(a, b = 2, c = 3, d = 4)
    super
  end
end
puts MultiChild.new.show(1)
puts MultiChild.new.show(1, 9)
puts MultiChild.new.show(1, 9, 8, 7)

# A parent optional beyond what the child supplies evaluates its own default.
class Defaults
  def run(a, b = "parent-default")
    "a=#{a} b=#{b}"
  end
end
class DefaultsChild < Defaults
  def run(a)
    super
  end
end
puts DefaultsChild.new.run("x")

# Keyword parameters are matched by NAME.
class KwParent
  def config(host:, port: 80)
    "#{host}:#{port}"
  end
end
class KwChild < KwParent
  def config(host:, port: 8080)
    super
  end
end
puts KwChild.new.config(host: "example.com")
puts KwChild.new.config(host: "example.com", port: 3000)

# A rest parameter forwards whole.
class RestParent
  def all(*args)
    args.inspect
  end
end
class RestChild < RestParent
  def all(*args)
    super
  end
end
puts RestChild.new.all(1, 2, 3)

# `super { ... }` passes a LITERAL BLOCK to the parent, which yields to it.
class BlockParent
  def run
    yield
  end

  def each_twice
    yield 1
    yield 2
  end
end
class BlockChild < BlockParent
  def run
    super { "from-child-block" }
  end

  def each_twice
    super { |v| puts "got #{v}" }
  end
end
puts BlockChild.new.run
BlockChild.new.each_twice

# With no literal block, the parent sees the CHILD's own block (forwarded).
class ForwardBlock < BlockParent
  def run
    super
  end
end
puts ForwardBlock.new.run { "from-caller" }

# A `super` block closes over the child's own locals -- `super()` here, since
# the parent's `run` takes no arguments and bare `super` would forward `tag`.
class ClosureBlock < BlockParent
  def run(tag)
    count = 0
    result = super() { count += 1; "#{tag}-#{count}" }
    "#{result} (count=#{count})"
  end
end
puts ClosureBlock.new.run("t")

# `super` through a module in the ancestor chain.
module Loud
  def speak
    super.upcase
  end
end
class Animal
  def speak
    "rawr"
  end
end
class Lion < Animal
  include Loud
end
puts Lion.new.speak
__END__
Parent(default)
Parent(given)
Parent(explicit)
Parent(ADA)
a=1 b=2 c=3 d=4
a=1 b=9 c=3 d=4
a=1 b=9 c=8 d=7
a=x b=parent-default
example.com:8080
example.com:3000
[1, 2, 3]
from-child-block
got 1
got 2
from-caller
t-1 (count=1)
RAWR
