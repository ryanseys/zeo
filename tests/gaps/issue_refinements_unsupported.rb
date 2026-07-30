# Refinements (`Module#refine` / `Kernel#using`) are unimplemented -- zeo
# raises NoMethodError for `refine` instead of scoping the refined methods to
# the `using` site. This file records the semantics an implementation must
# match, each line oracle-checked against ruby 4.0.6, because several are not
# what the documentation leads you to expect.
#
# THE RULE, as the oracle actually behaves: everything lexically AFTER the
# `using`, in the same file, sees the refinement -- including class and module
# bodies opened after it, and `def`s written after it. A `def` written BEFORE
# it does not. Dispatch is otherwise entirely ordinary: `send`, `respond_to?`
# and `Object#method` all honour it, and only `instance_methods` does not.
#
# THE PLAN (see the "compile-time lexical rewrite" task). `refine C do ... end`
# registers a hidden class holding the methods, keyed by (module, target).
# `using M` records an activation at its LEXICAL position -- which is not the
# `doc_order` execution numbering `defined?` uses, since a `def` body's
# lexical position is what matters here and its execution position is "when
# called". Each call site then carries the compile-time-constant set of
# refinements active at it:
#
#   * an ordinary `recv.m` whose name some active refinement defines emits a
#     receiver-class guard around the refined body, falling through to normal
#     dispatch -- correct whether or not the receiver's class is static;
#   * `send`/`public_send`/`respond_to?`/`method` at a site with ANY active
#     refinement pass that set to the runtime helper, which is exactly the
#     scope in which ruby honours them too.
#
# What must NOT happen is registering the refined method on the target class:
# that leaks it outside the `using` scope and puts it in `instance_methods`.
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
