# `Etc::Passwd` and its siblings are `Struct` subclasses in ruby. In zeo they
# are ordinary classes with hand-written readers, so the member WRITERS do
# not exist, `ancestors` does not include `Struct`, and the struct protocol
# rows (`to_a`, `to_h`, `members`, `values`, `each`, `[]`) are reported as
# the class's OWN rather than inherited from `Struct`.
#
# This is behavioural, not just reflection: `pw.name = "x"` raises
# NoMethodError where ruby assigns.
#
# The runtime already knows how to do this -- `register_compiled_struct`
# mints a real Struct subclass from a member list, and a user's
# `Struct.new(:a, :b)` goes through it. The builtin classes that CRuby
# defines with `rb_struct_define` need the same registrar instead of a
# `ruby_class!` table: `Etc::Passwd`, `Etc::Group`, `Process::Tms`, and the
# `Process::Status`-adjacent rows.
#
# The same shape explains the `to_a`/`to_s`/`inspect`/`values` rows reported
# as own on `Process::Tms`, and is worth checking against `Dir`'s `entries`
# and `to_s` when this is fixed -- those are a separate own-row misplacement
# (see the method census's owner-tag work).
#
# Oracle: a real Struct subclass.
require "etc"
pw = Etc.getpwuid
p Etc::Passwd.superclass.to_s
p Etc::Passwd.ancestors.include?(Struct)
p pw.respond_to?(:name=)
p Etc::Passwd.instance_methods(false).include?(:to_a)
p Process::Tms.superclass.to_s
