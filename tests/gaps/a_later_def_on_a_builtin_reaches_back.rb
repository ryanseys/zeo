# A later `def` reopening a BUILTIN class reaches BACK: a call written above
# the reopen answers with the reopened body. `redefs.rs` gives a user class's
# redefinitions a timeline, but a builtin's FIRST definition is a native row
# with no `Scope`, so there is no `method_history` entry to order against and
# the reopen is installed whole-program at startup.
#
# The obvious fix is the wrong one, and this is the part worth knowing before
# anyone re-scopes it. Reusing `HirNode::MethodRedefine` (the user-class
# mechanism) means `runtime_replace_method`, which calls `mark_live()` -- a
# GLOBAL gate that takes every send in the program off the flattened one-probe
# path and onto the per-ancestor walk. Gems override native methods routinely
# (activesupport alone does it dozens of times), so any program requiring one
# would pay that, to close a window most of them never observe.
#
# The mechanism that fits is a per-(class, name) switch instead: a generated
# `AtomicBool`, false until the reopen's document position stores true, read
# where the choice is made -- native row while false, the reopen's body once
# true. No global gate. `zeo_tramp!` expands to a non-capturing closure, so
# the inner trampoline is callable from a generated wrapper fn; the switch
# statement can reuse `MethodRedefine`'s node with an `is_builtin` branch in
# `codegen::stmt`, so no new HIR variant is needed.
#
# THE HALF THAT SKETCH MISSED, checked against the emitted Rust: both calls
# below bind STATICALLY to `__bm_Array::take_while`. They never reach a value
# row at all, so a dispatcher row would leave this program answering exactly
# as it does now. The name is NOT in `runtime_patches` either -- that records
# a RUNTIME (re)definition, and a compile-time builtin reopen is not one.
#
# So the switch has to be read at the CALL SITE. Putting the whole name on the
# dynamic path would work and is the expensive answer; the cheap one is a
# per-site branch, one relaxed load, because the compiler already knows where
# each site stands relative to the reopen:
#
#   textually after the reopen, same straight-line sequence -> bind to the
#     reopen directly, no load
#   anywhere else (above it, or inside a method that may run either side of
#     it) -> `if SWITCH { reopen } else { native }`
#
# That is a codegen change on the builtin-call fast path, which is where a
# naive frame push once cost `ruby_xor` 0.996s -> 3.8s. It needs its own pass
# with a bench number, not a corner of someone else's.
p [1, 2].take_while { true }
class Array
  def take_while
    :array_own
  end
end
p [1, 2].take_while { true }
