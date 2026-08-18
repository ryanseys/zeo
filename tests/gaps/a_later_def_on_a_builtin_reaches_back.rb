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
# The mechanism that fits is a per-(class, name) switch instead: register a
# DISPATCHER value row that reads a generated `AtomicBool` -- native row while
# false, the reopen's body once true -- and have the reopen's document position
# store `true`. One relaxed load per call of that one name, no global gate, and
# the name is already in `runtime_patches` so every site asks. `zeo_tramp!`
# expands to a non-capturing closure, so the inner trampoline is callable from
# a generated wrapper fn; the switch statement can reuse `MethodRedefine`'s
# node with an `is_builtin` branch in `codegen::stmt`, so no new HIR variant
# is needed.
p [1, 2].take_while { true }
class Array
  def take_while
    :array_own
  end
end
p [1, 2].take_while { true }
