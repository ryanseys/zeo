# A module's rows are MATERIALIZED onto every class that mixes it in, so a
# host's own flattened copy answers before the module is ever asked. A second
# `def` on the module wrote an overlay row nothing read, and every call --
# before the redefinition as well as after -- got the LAST body.
#
# The fix retires the host's copy, which sends the walk on to the module.
# A host with a `def` of its own keeps it.

module Kernel
  def helper = :first
end
p 5.helper
p "s".helper
p [].helper
p Object.new.helper
p helper
p 5.method(:helper).owner

module Kernel
  def helper = :second
end
p 5.helper
p "s".helper
p [].helper
p Object.new.helper
p helper

# A user module mixed into a BUILTIN.
module Shout
  def shout = "one"
end
class String
  include Shout
end
p "x".shout
p String.instance_method(:shout).owner
module Shout
  def shout = "two"
end
p "x".shout

# A user module mixed into a user class, and a host with its OWN def.
module M
  def m_helper = :one
end
class C
  include M
end
class D
  include M
  def m_helper = :own
end
c = C.new
p c.m_helper
p D.new.m_helper

module M
  def m_helper = :two
end
p c.m_helper
p D.new.m_helper
p C.instance_method(:m_helper).owner
p D.instance_method(:m_helper).owner
