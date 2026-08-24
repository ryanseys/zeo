# A `Method` or `UnboundMethod` captured before a reopen keeps answering for
# the definition it was taken from. zeo's handle carries only `(class, name)`
# and re-reads the live tables, so it follows the redefinition.
#
# This is NOT the redefinition-window shape, which is now fixed on both the
# body and the reflection channel
# (tests/a_redefinition_window_reports_its_own_body.rb): every table below
# answers correctly at the moment it is asked. What diverges is the HANDLE --
# CRuby's `Method` holds the `rb_method_entry_t` it was built from, so it is a
# snapshot and zeo's is a lookup.
#
# The fix is a definition identity on the handle: the redefinition rows already
# exist per body (`ProgramDesc::redef_metas`), so a handle taken in a window
# could record which row was live and reflect through that one. Calling it is
# the same question and the same answer -- CRuby runs the captured body too.
class U
  def u = 1
end
first = U.instance_method(:u)
class U
  def u(a) = 2
end
p [first.arity, U.instance_method(:u).arity]
p first.parameters
p U.instance_method(:u).parameters

class B
  def b = "b1"
end
m = B.new.method(:b)
class B
  def b = "b2"
end
p [m.arity, m.call, B.new.b]
