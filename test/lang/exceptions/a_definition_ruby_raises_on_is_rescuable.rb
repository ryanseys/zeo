# `superclass mismatch` is an EXCEPTION in ruby, raised at the definition -- not
# a broken program. A `begin ... rescue TypeError` around one catches it, prints,
# and carries on, so a program that does that has to compile and do the same.
#
# Unrescued, zeo refuses the compile instead of emitting a program that aborts:
# same problem, named earlier. That split is the whole point of this file -- the
# rescued half is what a compile-time-only rejection would have made impossible.
#
# The KIND mismatch at the bottom is the same rule with a two-line message, and
# both lines were wrong. `unmatched_redefinition` (vm_insnhelper.c) names the
# LEAF -- `rb_id2str(id)`, the id off the cpath -- so `module Outer::Inner`
# reports `Inner`, where zeo reported the qualified `Outer::Inner`. Then it
# appends the previous definition's position, read from
# `rb_const_source_location_at`: the point the CONSTANT was created, which is
# the first declaring site, so a reopen leaves the first line standing. zeo
# omitted that line entirely.

class A; end
class B < A; end

# A cycle: `A < B` while `B < A`. Ruby raises rather than closing the loop --
# and zeo must not build it either, because a superclass chain that points at
# itself is an infinite walk for every pass that follows it.
begin
  class A < B; end
rescue TypeError => e
  p e.message
end
p A.superclass
p B.superclass

# A plain conflict: two different superclasses for one name.
class D < String; end
begin
  class D < Array; end
rescue TypeError => e
  p e.message
end
p D.superclass

# The definition RAISED, so it changed nothing -- the class is exactly what it
# was, and the program keeps running.
p D.new.class
p B.new.is_a?(A)

# --- the KIND mismatch, both directions -------------------------------------
begin
  module D; end
rescue TypeError => e
  p e.message
end
p D.superclass

module M; end
begin
  class M; end
rescue TypeError => e
  p e.message
end
p M.class

# The name reported is the LEAF, and the position is the first declaring site.
module Outer
  class Inner; end
end
begin
  module Outer::Inner
  end
rescue TypeError => e
  p e.message
end

# A REOPEN does not restamp the position: the line below is the one above.
class Reopened; end
class Reopened; end
begin
  module Reopened; end
rescue TypeError => e
  p e.message
end

# A builtin's constant carries no recorded position, and ruby prints the line
# anyway -- with both fields empty.
begin
  module String; end
rescue TypeError => e
  p e.message
end

p :after
__END__
"superclass mismatch for class A"
Object
A
"superclass mismatch for class D"
String
D
true
"D is not a module\nlang/exceptions/a_definition_ruby_raises_on_is_rescuable.rb:33: previous definition of D was here"
String
"M is not a class\nlang/exceptions/a_definition_ruby_raises_on_is_rescuable.rb:54: previous definition of M was here"
Module
"Inner is not a module\nlang/exceptions/a_definition_ruby_raises_on_is_rescuable.rb:64: previous definition of Inner was here"
"Reopened is not a module\nlang/exceptions/a_definition_ruby_raises_on_is_rescuable.rb:74: previous definition of Reopened was here"
"String is not a module\n:: previous definition of String was here"
:after
