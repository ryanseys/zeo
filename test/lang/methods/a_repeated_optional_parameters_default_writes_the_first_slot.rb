# A repeated `_` parameter binds the FIRST slot for reads, and
# `test/lang/blocks/a_repeated_underscore_parameter_binds_the_first_slot.rb` pins that.
# One shape inside the same rule is still wrong: a repeated OPTIONAL whose
# DEFAULT runs.
#
# CRuby compiles an optional parameter's default as an assignment to the NAME,
# not to the slot -- so when the second `_ = 2` fires it writes the local the
# first `_` owns. Oracle (ruby 4.0.6):
#
#   def f(_ = 1, _ = 2); _; end
#   f       # => 2   -- both defaults run, the second one wins
#   f(9)    # => 2   -- the first slot took 9, then the second default wrote 2
#   f(9, 8) # => 9   -- NO default ran, so nothing overwrote the first slot
#
# zeo binds each slot independently (`SlotIdents`), so a later slot never
# writes the name and every read answers the first slot. Measured:
#
#            CRuby 4.0.6   zeo
#   both_default              2      1
#   both_default(9)           2      9
#   both_default(9, 8)        9      9   <- agrees: no default ran
#   required_then_optional(1) 5      1
#   ...(1, 2)                 1      1   <- agrees: no default ran
#   leading_then_default(0,7) [0, 3] [0, 7]
#
# Only the DEFAULT path diverges, which is why the two agreeing lines are here
# too: they are what says the rest of the rule is right.
#
# The fix is for a duplicate optional's default branch to assign the owning
# ident as well as its own slot. That needs the owning slot to be `mut` and in
# scope at that point, which it is not for a required parameter bound by the
# Rust signature -- so it is a signature change, not a local one.
def both_default(_ = 1, _ = 2)
  _
end
p both_default
p both_default(9)
p both_default(9, 8)

def required_then_optional(_, _ = 5)
  _
end
p required_then_optional(1)
p required_then_optional(1, 2)

def leading_then_default(a, _ = 2, _ = 3)
  [a, _]
end
p leading_then_default(0, 7)
__END__
2
2
9
5
1
[0, 3]
