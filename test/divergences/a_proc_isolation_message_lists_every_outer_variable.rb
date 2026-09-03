# `Ractor.new { .. }` over an outer local raises CRuby's own ArgumentError (see
# `a_ractor_block_isolates_at_runtime.rb`), and only the VARIABLE LIST in the
# message differs. Two rules produce CRuby's list, and neither is mirrorable
# from a lowered HIR:
#
# 1. ORDER is the ENCLOSING SCOPE's declaration order -- not the block's use
#    order, not alphabetical. `zz = 1; aa = 2; mm = 3` then
#    `Ractor.new { zz + mm + aa }` reports `(zz, aa, mm)`, because CRuby walks
#    the outer iseq's local table and that table is declaration-ordered. zeo has
#    the capture SET (`Captures::locals`, a hash set) and no record of where
#    each name was first bound in the enclosing scope.
#
# 2. The SET is what the compiled iseq actually READS. `{ aa; aa; zz }` reports
#    only `(zz)`: the two bare `aa` statements are value-less in non-tail
#    position, and CRuby's peephole deletes them before `rb_proc_isolate` walks
#    the instructions. zeo's HIR keeps the reads, so it would name `aa` too.
#
# Rule 2 is the harder half -- matching it means modelling a peephole zeo does
# not have. Rule 1 alone would need the enclosing scope's declaration ORDER
# carried to the `Ractor.new` emission site: `Captures::outer_assigned_at`
# already records `(file, offset)` per name, but it is computed for the BLOCK's
# subtree rather than the enclosing scope's, `Ctx` keeps only the name set
# (`captured_locals`), and params are absent from that map besides -- and they
# come first in CRuby's local table.
#
# Every row in the gem corpus names exactly ONE outer variable, where the two
# rules cannot disagree. So this is a message-text divergence, not a
# behavioural one: the same programs raise, with the same class.
#
# --- ruby 4.0.6 answers ---
# "can not isolate a Proc because it accesses outer variables (zz, aa, mm)."
# "can not isolate a Proc because it accesses outer variables (zz)."
# "can not isolate a Proc because it accesses outer variables (only)."

# A DECIDED DIVERGENCE in message TEXT only. `Ractor.new { ... }` over an
# outer local raises CRuby's own ArgumentError (see
# `a_ractor_block_isolates_at_runtime.rb`); the VARIABLE LIST in the message
# is zeo's. The golden records ZEO's output and the `.divergence` sidecar
# carries the reason and ruby's answer.
#
# Two rules behind CRuby's list, neither mirrorable from a lowered HIR:
#
# 1. ORDER is the ENCLOSING SCOPE's declaration order, not the block's use
#    order and not alphabetical. `zz = 1; aa = 2; mm = 3` then
#    `Ractor.new { zz + mm + aa }` reports `(zz, aa, mm)` -- CRuby walks the
#    outer iseq's local table, which is declaration-ordered. zeo has the
#    capture SET (`Captures::locals`, a hash set) and no record of where each
#    name was first bound in the enclosing scope, so it reports one name.
#
# 2. The SET is what the compiled iseq actually reads. `{ aa; aa; zz }` reports
#    only `(zz)`: the two bare `aa` statements are value-less in non-tail
#    position and CRuby's peephole deletes them before `rb_proc_isolate` walks
#    the instructions. zeo's HIR keeps the reads, so it would name `aa` too.
#
# Rule 2 is the harder half -- matching it means modelling a peephole zeo does
# not have. Rule 1 alone would need the enclosing scope's declaration ORDER
# carried to the `Ractor.new` emission site: `Captures::outer_assigned_at`
# already records `(file, offset)` per name, but it is computed for the BLOCK's
# subtree, not the enclosing scope's, and `Ctx` keeps only the name set
# (`captured_locals`). Params are absent from that map besides, and they come
# first in CRuby's local table.
#
# Every row in the gem corpus names exactly ONE outer variable, where the two
# rules cannot disagree, which is why this is a message-text gap and not a
# behavioural one.
Warning[:experimental] = false

zz = 1
aa = 2
mm = 3

begin
  Ractor.new { zz + mm + aa }
rescue ArgumentError => e
  p e.message
end

begin
  Ractor.new { aa; aa; zz }
rescue ArgumentError => e
  p e.message
end

# The single-variable case, which zeo already gets exactly right.
only = 9
begin
  Ractor.new { only }
rescue ArgumentError => e
  p e.message
end
__END__
"can not isolate a Proc because it accesses outer variables (aa)."
"can not isolate a Proc because it accesses outer variables (aa)."
"can not isolate a Proc because it accesses outer variables (only)."
