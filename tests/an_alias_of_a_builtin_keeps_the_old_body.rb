# `alias_method :old, :m` on a BUILTIN class, followed by `def m`, makes `old`
# resolve to the NEW body. The wrapper then calls itself and the program dies
# with SystemStackError.
#
# This is the standard monkeypatch-wrap idiom -- capture the original under a
# second name, redefine, call through. Net::HTTP debugging, test doubles and
# a great deal of gem code are written this way, so the failure is not exotic.
#
# The cause is the same one `a_later_def_on_a_builtin_reaches_back.rb`
# records, seen from the other side. A builtin's first definition is a native
# row with no `Scope`, so `alias_method` cannot copy a BODY -- it can only
# record "this name forwards to that name". The later `def` then replaces what
# that name resolves to, and the alias follows it. On a USER class the alias
# copies the method entry, so the same program is correct there.
#
# The fix is what the alias records: it must bind the method ENTRY that is
# live at the alias's document position, not the name. `runtime_meta` already
# keys entries per position for user classes; a builtin needs its native row
# materialized into an entry the alias can hold. Doing that for every builtin
# alias would cost; doing it only where an alias names a builtin row is the
# shape that fits.
#
# The second program below is the same bug with no redefinition at all: an
# alias taken BEFORE a reopen must still answer the original.

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
