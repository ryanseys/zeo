# `Module#const_added` -- ruby announces a constant the moment it becomes
# readable, on the module it was set on, with the constant's own leaf name.
#
# Unlike the `method_*` family this one has no singleton rerouting, and its
# order against the other two class hooks is fixed: `const_added` for the name,
# THEN `inherited`, THEN the class body.

puts "== a plain write, a class, a module"
module Outer
  def self.const_added(n) = puts("Outer const_added #{n}")
  def self.inherited(s) = puts("Outer inherited #{s}")

  X = 1
  class Inner
    puts "  (inner body runs after its name is announced)"
  end
  module M; end

  # A class minted by an expression is announced by the write that names it.
  S = Struct.new(:a)
  D = Data.define(:b)
  K = Class.new
end

puts "== a reopen declares nothing"
module Outer
  class Inner
    puts "  (inner reopened -- no announcement)"
  end
  Z = 9
end

puts "== every assignment announces, re-assignment included"
Outer.send(:remove_const, :Z)
Outer::Z = 10

puts "== const_added, then inherited, then the body"
class Base
  def self.const_added(n) = puts("Base const_added #{n}")
  def self.inherited(s) = puts("Base inherited #{s}")
end
class Base
  class Child < Base
    puts "  (child body)"
  end
end

puts "== the runtime shapes"
module Runtime
  def self.const_added(n) = puts("Runtime const_added #{n}")
end
Runtime.const_set(:VIA_SET, 1)
# An autoload announces at DECLARATION, not when the file loads -- and on every
# declaration, re-declaring the same name included.
Runtime.autoload(:LAZY, "some/file")
Runtime.autoload(:LAZY, "some/file")

puts "== an explicit scope announces on that scope"
Outer::FROM_OUTSIDE = 5

puts "== a module with no hook stays silent"
module Quiet
  QUIET_CONST = 1
  class QuietInner; end
end
p Quiet.constants.sort
__END__
== a plain write, a class, a module
Outer const_added X
Outer const_added Inner
  (inner body runs after its name is announced)
Outer const_added M
Outer const_added S
Outer const_added D
Outer const_added K
== a reopen declares nothing
  (inner reopened -- no announcement)
Outer const_added Z
== every assignment announces, re-assignment included
Outer const_added Z
== const_added, then inherited, then the body
Base const_added Child
Base inherited Base::Child
  (child body)
== the runtime shapes
Runtime const_added VIA_SET
Runtime const_added LAZY
Runtime const_added LAZY
== an explicit scope announces on that scope
Outer const_added FROM_OUTSIDE
== a module with no hook stays silent
[:QUIET_CONST, :QuietInner]
