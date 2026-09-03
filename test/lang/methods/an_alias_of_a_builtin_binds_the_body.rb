# `alias new old` captures the METHOD, not the name: a later `def old`
# replaces the original without disturbing the alias.
#
# The alias of a builtin used to be a name indirection resolved at call
# time, so the standard wrap idiom -- alias the primitive away, redefine
# it, call the alias from the new body -- recursed until the stack ran
# out. It is how rubygems installs its own `Kernel#require`.

module Kernel
  alias orig_p p
  def p(*a)
    orig_p(:wrapped, *a)
  end
end
p 1

# The same shape across two bodies, where the redefinition cannot even be
# seen from the body that wrote the alias.
module Kernel
  alias orig_pr print
end
module Kernel
  def print(*a)
    orig_pr("[", *a, "]\n")
  end
end
print "two bodies"

# A builtin CLASS, reached through an instance rather than a universal.
class String
  alias orig_upcase upcase
  def upcase = "shout(#{orig_upcase})"
end
puts "hi".upcase

# A user class, whose aliased primitive comes from an ancestor's table
# rather than its own.
class Widget
  alias orig_inspect inspect
  def inspect = "Widget!"
end
w = Widget.new
puts w.inspect
puts w.orig_inspect.start_with?("#<Widget:0x")

# The control: an alias of a USER method already bound its body.
class Counter
  def tick = "first"
  alias orig_tick tick
  def tick = "second"
end
puts Counter.new.tick
puts Counter.new.orig_tick
__END__
:wrapped
1
[two bodies]
shout(HI)
Widget!
true
second
first
