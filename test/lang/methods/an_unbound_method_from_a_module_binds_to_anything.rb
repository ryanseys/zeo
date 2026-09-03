# Since ruby 3.0 the bind check reads the OWNER, and a MODULE owner imposes no
# check at all: the receiver does not have to have the module in its ancestry.
# zeo read the class the method was FETCHED from and applied the class rule to
# every owner, so both halves of that were wrong.
M = Module.new { def nest = 1 }
p M.instance_method(:nest).bind(Object.new).call
p M.instance_method(:nest).bind_call(Object.new)

module Named
  def named_one = :named
end
p Named.instance_method(:named_one).bind(Object.new).call
p Named.instance_method(:named_one).bind(String).call

# Fetched from a class that mixes it in, the owner is still the module.
module Mixin
  def mixed = :m
end
class Host
  include Mixin
end
um = Host.instance_method(:mixed)
p um.owner
p um.bind(Object.new).call
p um.bind_call(Object.new)

# A CLASS owner keeps the rule, and the rule reads the owner rather than the
# class it was fetched from: this one came off `B` and binds an `A`.
class A
  def x = 1
end
class B < A
end
b = B.instance_method(:x)
p b.owner
p b.bind(A.new).call

# What must still refuse.
begin
  A.instance_method(:x).bind(Object.new)
rescue TypeError => e
  p e.message
end
class S
  def self.only = 1
end
begin
  S.singleton_class.instance_method(:only).bind(Object.new)
rescue TypeError => e
  p e.message
end
p S.singleton_class.instance_method(:only).bind(S).call
__END__
1
1
:named
:named
Mixin
:m
:m
A
1
"bind argument must be an instance of A"
"singleton method called for a different object"
1
