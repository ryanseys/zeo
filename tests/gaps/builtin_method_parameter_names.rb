# `Method#parameters` on a BUILTIN row answers the anonymous descriptor derived
# from its arity -- `[[:req]]` where ruby says `[[:req, :path]]`.
#
# LARGELY CLOSED 2026-08-21. The mechanism landed and 96 of the 142 diverging
# rows went with it; what is left is 46, and it is left DELIBERATELY.
#
# What the mechanism is: the DSL spells a row's signature per NAME
# (`params "fmt, buffer: nil"`), in ruby's own `def` spelling, and the ARITY
# falls out of it (`zeo_dsl::signature_arity`) -- so the two answers cannot
# disagree for a row that carries one, and an `arity N` override becomes
# unnecessary wherever a spelling is written. The proc-macro bakes the
# descriptor into a fourth generated lookup beside `lookup_arity`, reached
# through a `params` field on `MethodTable`. A row that spells nothing keeps
# the arity-derived descriptor, so annotating is additive.
#
# It also gave the DSL a KEYWORD spelling, which it had none of. That is not an
# oversight being corrected: a native body receives a keyword inside the
# options Hash it already takes as one positional slot, so what the body
# RECEIVES and what ruby REPORTS are genuinely different lists. `params` is
# metadata, not a binding, which is why it can spell one at all.
#
# THE 46 THAT REMAIN are Pathname (41) and Kernel (5), and they are exactly the
# clusters G8's corelib closes BY CONSTRUCTION -- `pathname_builtin.rb` and
# `kernel.rb` are vendored from CRuby with their real signatures in them.
# Hand-annotating them now would write the same 46 answers twice and leave two
# sources to drift. The scoping decision (2026-08-21, user-directed) was "only
# the clusters corelib won't reach", and this is the other side of it.
#
# Two things the pass fixed that were not on the list, both found by measuring
# rather than by reading the ledger:
#
#   Every hand-registered `Exception` row reached no arity table, so zeo's two
#   catch-alls for an unknown row disagreed with each other -- `-1` through
#   `#arity` and `[]` through `#parameters`. 34 arity divergences came from
#   that one cause, and the whole-surface arity diff is now ZERO. The kinds
#   ride in `BY_OWNER` beside the ownership marks so the two cannot drift.
#
#   The walk that finds a spelled signature has to STOP at the ancestor that
#   owns the row. Continuing (a `?` inside `find_map` does) answered
#   `Enumerable#to_set`'s spelling for `Range#to_set`, which ruby declares
#   separately.
#
# The line below is Kernel's, so this file stays a gap until G8.
p method(:require).parameters
