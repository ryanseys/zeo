# The cycle this program builds on purpose, and the census the
# `ZEO_RT_GCCHECK=1` leg gates against. A cycle alive at exit is not a
# defect: it is a ring the program never broke. What the leg gates is a
# CHANGE to the line below.
# 
# PrettyPrint's own graph, reached through `pp`. See pp_pretty_print.rb.gccheck.
#@ zeo-env: ZEO_RT_LEAKCHECK=1
#@ gccheck: cycle leak: 11 objects (Array x7, PP x1, PrettyPrint::Group x1, PrettyPrint::GroupQueue x1, Proc x1)
# Compiled code must not hand `zeo_rt_release` a stack slot that holds no
# live value. This program did, and `ZEO_RT_LEAKCHECK=1` reported tag byte
# 176 -- not a value tag (they run 0..33) and not the poison byte a released
# slot is stamped with (0xFF). The release then ran `drop_in_place` over
# whatever the stack had there.
#
# THE CAUSE, and it is not what the symptom suggests. `Fx::new_value_slot`
# used to emit its three nil words into the block that was CURRENT when the
# slot was created. Seven of its eight callers sit in a function prologue.
# The eighth, `bind_shadow` in `clif/iter.rs`, runs inside a fused loop's
# inline arm -- behind the receiver's tag test and `iter_inline_ok_for` --
# and inserts the slot into `fx.locals`, which `release_locals` walks
# UNCONDITIONALLY from both the normal exit and the landing.
#
# Two facts made it wider than the fused-loop guard:
#
#   * `fx.land` is one block, created in `Fx::new`, that every `Fx::fallible`
#     branches to. A raise anywhere ahead of the loop reaches the epilogue
#     with the slot un-zeroed, so the UNGUARDED `n.times` and range-`each`
#     splices were exposed too.
#   * `new_cell_local` had the same defect over an 8-byte pointer slot, which
#     is worse: the epilogue loaded it and passed it to `zeo_rt_cell_release`.
#
# THE FIX. Every slot is initialised in the ENTRY block. `new_value_slot` and
# `new_cell_slot` record the slot; `Fx::drain_slot_inits` emits the stores at
# the top of the entry block just before the builder is finalized. Same
# instruction count, and no path can reach an epilogue with an uninitialised
# slot. A debug assertion pins the invariant: every slot in `fx.locals` was
# created through one of those two helpers.
#
# `clif/verify.rs` passed throughout and could not have caught this -- it is
# a path-insensitive site counter, and an uninitialised slot is not an
# ownership event.
#
# The `.leakcheck` sidecar is load-bearing: without `ZEO_RT_LEAKCHECK=1` the
# program prints the right answer either way.
#
# Oracle: the program prints its line and exits 0.
require "pp"

class Plain
end

PP.pp(Plain.new, +"")
puts "done"
__END__
done
