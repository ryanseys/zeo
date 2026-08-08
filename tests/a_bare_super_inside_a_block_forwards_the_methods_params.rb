# A bare `super` forwards the enclosing method's parameters as currently
# bound. Inside a BLOCK that is invisible to any walk of the block's own
# syntax: the forwarding argument list is synthesized when the call is emitted,
# so nothing in the block mentions the parameters at all.
#
# zeo's capture analysis walks local reads, found none, and captured nothing.
# The block's `move` closure then consumed the enclosing parameter outright,
# and every later use of it in the method was a borrow-after-move --
# `require "active_model"` failed to build with 14 of these, over `name`,
# `options` and `url_safe`.
class Base
  def greet(name, options = {})
    "base:#{name}:#{options[:k]}"
  end
end

class Child < Base
  def greet(name, options = {})
    got = [1].map { super }
    "#{got.first}|after:#{name}:#{options[:k]}"
  end
end

p Child.new.greet("x", k: 1)

# `super` forwards the parameters' CURRENT values, not the ones passed in, and
# a block sees the reassignment the same way the method body does.
class Reassigning < Base
  def greet(name, options = {})
    name = name.upcase
    inner = [1].map { super }.first
    "#{inner}|still:#{name}"
  end
end

p Reassigning.new.greet("y", k: 2)

# Every parameter kind forwards, and the method keeps using each one after.
class WideBase
  def call(a, b = :bee, *rest, k:, m: :em, **opts)
    [a, b, rest, k, m, opts]
  end
end

class Wide < WideBase
  def call(a, b = :bee, *rest, k:, m: :em, **opts)
    from_block = [1].map { super }.first
    [from_block, a, b, rest, k, m, opts]
  end
end

p Wide.new.call(1, :two, 3, 4, k: :kay, z: 9)

# The parameter is still a live, mutable local afterwards -- the closure took a
# copy, not the binding.
class Mutating < Base
  def greet(name, options = {})
    2.times { super }
    name << "!"
    name
  end
end

p Mutating.new.greet(+"z")

# A block that DOES name a parameter still works, and one whose own parameter
# shadows the method's must not confuse the two.
class Shadowing < Base
  def greet(name, options = {})
    seen = ["inner"].map { |name| name }
    "#{[1].map { super }.first}|#{seen.first}|#{name}"
  end
end

p Shadowing.new.greet("outer", k: 3)

# A nested block reaches the same parameters through two closures.
class Nested < Base
  def greet(name, options = {})
    got = [1].map { [2].map { super }.first }.first
    "#{got}|#{name}"
  end
end

p Nested.new.greet("deep", k: 4)
