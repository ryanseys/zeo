# A bare `super` inside a `define_method` body is an error in ruby:
#
#     RuntimeError: implicit argument passing of super from method defined by
#     define_method() is not supported. Specify all arguments explicitly.
#
# zeo runs it as though `super()` had been written, so a program that ruby
# refuses silently does something -- and something subtly different from what
# a bare `super` means anywhere else, since a zsuper forwards the CURRENT values
# of the method's parameters and a `define_method` body has no parameter list to
# forward from.
#
# Ruby refuses rather than guessing precisely because the answer is ambiguous.
# Accepting it means a `define_method` wrapper written by a macro forwards
# nothing where the author expected the arguments to carry through, which fails
# later at the callee rather than here.
#
# The explicit form is fine and must stay working (the first case below).

class Base
  def who(*args) = "base#{args.inspect}"
end

explicit = Class.new(Base) { define_method(:who) { |*a| "dm(" + super(*a) + ")" } }
p explicit.new.who(1, 2)

zsuper = Class.new(Base) { define_method(:who) { super } }
begin
  p zsuper.new.who
rescue RuntimeError => e
  puts "#{e.class}: #{e.message}"
end

with_params = Class.new(Base) { define_method(:who) { |a| super } }
begin
  p with_params.new.who(1)
rescue RuntimeError => e
  puts "#{e.class}: #{e.message}"
end

# A bare `super` in an ordinary `def` is fine.
ordinary = Class.new(Base) { def who(*args) = "def(" + super + ")" }
p ordinary.new.who(3)
