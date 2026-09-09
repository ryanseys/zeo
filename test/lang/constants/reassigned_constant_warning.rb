# Assigning a constant that already has a value warns TWICE on stderr -- once
# at the new assignment, once naming where the previous one was. zeo assigns
# silently, so the one diagnostic that catches a constant clobbered by a
# second file, or by a name a library did not expect to own, never appears.
#
# The location half is in place: `test/lang/modules/const_source_location_of_a_module_constant.rb`
# records where every constant was assigned, and the warning's second line is
# exactly that record read back. What is missing is the check at the write --
# `constants.rs`'s `const_set` has no "was it already there" test, and
# `warning.rs` is the channel it would report through.
#
# CRuby is deliberate about which writes are quiet: a `remove_const` first
# clears the record, and a `class`/`module` reopen creates nothing, so neither
# warns. Only a genuine second assignment does.

D = 1
D = 2
p D

module M
  E = 1
  E = 2
end

F = 3
Object.send(:remove_const, :F)
F = 4

class C; end
class C; end

G = 5
Object.const_set(:G, 6)
__END__
2
#@ stderr
lang/constants/reassigned_constant_warning.rb:17: warning: already initialized constant D
lang/constants/reassigned_constant_warning.rb:16: warning: previous definition of D was here
lang/constants/reassigned_constant_warning.rb:22: warning: already initialized constant M::E
lang/constants/reassigned_constant_warning.rb:21: warning: previous definition of E was here
lang/constants/reassigned_constant_warning.rb:33: warning: already initialized constant G
lang/constants/reassigned_constant_warning.rb:32: warning: previous definition of G was here
