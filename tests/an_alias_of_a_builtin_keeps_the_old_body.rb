# `alias_method :old, :m` on a BUILTIN class, followed by `def m`, keeps
# `old` on the ORIGINAL body -- the standard monkeypatch-wrap idiom, which
# Net::HTTP debugging, test doubles and a great deal of gem code are written
# with. It once made the wrapper call itself until the stack ran out.
#
# A builtin's first definition is a native row with no `Scope`, so the alias
# cannot clone a body the way a user-class alias does; the row it registers
# names the source instead. Dispatch answers that row from the native table
# rather than by re-sending the name, so a later `def` cannot capture it.
#
# The rows here name the receiver's OWN class. The chain case -- a universal,
# or a user class inheriting the primitive -- is
# `an_alias_of_a_builtin_binds_the_body.rb`.
#
# The second program below is the same rule with no redefinition at all: an
# alias taken BEFORE a reopen still answers the original.

class String
  alias_method :zeo_gap_old_upcase, :upcase
  def upcase = "wrapped(#{zeo_gap_old_upcase})"
end

puts "ab".upcase
puts "ab".zeo_gap_old_upcase

class Array
  alias_method :zeo_gap_old_first, :first
end

class Array
  def first(*) = "replaced"
end

puts [1, 2].first.inspect
puts [1, 2].zeo_gap_old_first.inspect
