# `Etc::Passwd`, `Etc::Group` and `Process::Tms` are real `Struct` subclasses,
# as `rb_struct_define` makes them in CRuby -- so the whole struct protocol is
# INHERITED and each class supplies only its members and accessors.
#
# zeo used to make them ordinary classes with hand-written readers and a
# hand-rolled copy of the protocol beside them (`ext/etc/mod.rs` carried a
# `StructRow` trait with its own `to_a`/`to_h`/`each`/`[]`/`inspect`). That
# lost the member WRITERS entirely -- `pw.name = "x"` raised NoMethodError --
# reported the protocol rows as the class's own, kept `Struct` out of
# `ancestors`, and left `==` comparing by identity where a Struct compares by
# value.
#
# The bridge is `RubyObject::hidden_ivar_get`/`_set`: a member by INDEX, which
# is how `slots_of`/`slot_get`/`slot_set` read one. Implementing those two over
# a slot vector is the whole of what makes a native payload a Struct -- every
# other row then comes from `Struct` itself, including the ones no one had
# written (`dig`, `size`, `values_at`, `each_pair`, `deconstruct`, `Marshal`).
#
# `Etc::Passwd.each` and `Etc::Group.each` were missing outright; they are the
# database cursors, the same walk `Etc.passwd`/`Etc.group` run with a block.

require "etc"

pw = Etc.getpwuid
p Etc::Passwd.superclass.to_s
p Etc::Passwd.ancestors.include?(Struct)
p Etc::Passwd.ancestors.include?(Enumerable)
p pw.respond_to?(:name=)
p Etc::Passwd.instance_methods(false).include?(:to_a)
p Process::Tms.superclass.to_s

# A Struct's `==`/`eql?` compare BY VALUE.
p [pw == Etc.getpwuid, pw.eql?(Etc.getpwuid), pw.equal?(Etc.getpwuid)]

# The inherited protocol, none of which is written per class.
t = Process.times
p [t.size, t.to_a.size, t.to_h.keys.map(&:to_s), t.members.map(&:to_s)]
p [t.each.to_a.size, t.each_pair.to_a.size, t.deconstruct.size]
p [t.dig(0).class.to_s, t.values_at(0, 1).size, t.select { true }.size]
p t.to_s.start_with?("#<struct Process::Tms ")
p Marshal.load(Marshal.dump(t)).to_a == t.to_a

# The writers, by name and by index.
pw.name = "zzz"
p pw.name
pw[1] = "yyy"
p pw.passwd

# The class methods a Struct subclass carries in its OWN singleton.
p Etc::Passwd.singleton_methods(false).sort.map(&:to_s)
p Process::Tms.singleton_methods(false).sort.map(&:to_s)
p Etc::Passwd["a"].to_a.first
p Etc::Passwd.keyword_init?

# The database cursors.
n = 0
Etc::Passwd.each { |_row| n += 1 }
p n > 0
