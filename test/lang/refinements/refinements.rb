# Refinements (`Module#refine` / `Kernel#using`), every line oracle-checked
# against ruby 4.0.6 -- several are not what the documentation leads you to
# expect.
#
# THE RULE, as the oracle behaves: everything lexically AFTER the `using`, in
# the same file, sees the refinement -- including class and module bodies
# opened after it, and `def`s written after it. A `def` written BEFORE it does
# not. Dispatch is otherwise entirely ordinary: `send`, `respond_to?` and
# `Object#method` all honour it, and only `instance_methods` does not.
#
# `refine C do ... end` puts its methods in a hidden holder MODULE, never on
# the target class -- registering them on the target would leak them past the
# `using` scope and into `instance_methods`. `using M` records the lexical
# byte range it covers, and every call site inside one routes through
# `refined_send`, which tries the holders and then falls back to an ordinary
# send.
module M
  refine String do
    def shout = upcase + "!"
    def size = 999
  end
  refine Integer do
    def double = self * 2
  end
end

# A refinement is invisible before the `using`, and to a `def` written above it.
def early(s)
  begin
    s.shout
  rescue NoMethodError
    :not_visible
  end
end
begin
  "a".shout
rescue NoMethodError => e
  p [:before, e.class]
end

using M

p early("x")
p "hi".shout
p "hi".size
p 3.double
p [1, 2].map { |n| n.double }

# A `def` and a class body written after the `using` both see it.
def helper(s) = s.shout
p helper("ok")
class Holder
  def call(s) = s.shout
end
p Holder.new.call("z")

# Ordinary dispatch honours it; reflection over the class does not.
p "hi".respond_to?(:shout)
p "hi".send(:shout)
p "hi".method(:shout).owner.class
p String.instance_methods.include?(:shout)

# A non-literal receiver, so the guard cannot lean on a static class.
x = "dyn"
p x.shout

# `using` inside a module body scopes to that body.
module Inner
  using M
  def self.go(s) = s.shout
end
p Inner.go("q")
p M.class

# The holder the refined methods live in is a constant of nobody's, and
# `Refinement` is an ordinary `Module` subclass.
p M.constants
p Refinement.superclass

# A second `using` stacks onto the first rather than replacing it.
module N
  refine Array do
    def second = self[1]
  end
end
using N
p [1, 2, 3].second
p "still".shout

# `super` in a refined method reaches what the refinement overrode, and an
# operator refines like any other name.
class Widget
  def label = "plain"
end
module P
  refine Widget do
    def label = "refined " + super
  end
  refine Integer do
    def +(other) = 999
  end
  # A refinement is active inside its own block, so a sibling refined method
  # is reachable by bare name.
  refine String do
    def bang = self + "!"
    def twice = bang + bang
  end
end
using P
p Widget.new.label
p 1 + 2
p "a".twice
__END__
[:before, NoMethodError]
:not_visible
"HI!"
999
6
[2, 4]
"OK!"
"Z!"
true
"HI!"
Refinement
false
"DYN!"
"Q!"
Module
[]
Module
2
"STILL!"
"refined plain"
999
"a!a!"
