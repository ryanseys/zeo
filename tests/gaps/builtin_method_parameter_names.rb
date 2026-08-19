# `Method#parameters` on a BUILTIN row answers the anonymous descriptor
# derived from its arity -- `[[:req]]` where ruby says `[[:req, :path]]`.
# zeo reports a parameter NAME on no builtin row at all.
#
# MEASURED against the 4,464 rows of conformance/builtin-arity.tsv (whose 8th
# column already carries ruby's own descriptors): 265 rows diverge. 100 of
# them differ only in the names; the other 165 differ in KIND, and 76 of those
# want a KEYWORD parameter, which the DSL cannot spell at all. The dense
# clusters are ObjectSpace (50), Pathname (50), IO (40), Ractor (17),
# RubyVM::YJIT (16), Kernel (15) and Monitor (14).
#
# TWO SHAPES OF FIX, and the choice is the work:
#
#   the DSL grows parameter metadata -- names, and a keyword spelling it has
#   no syntax for. One source of truth, so `arity` and `parameters` agree by
#   construction, and the arity ratchet already gates half of it. ~265 row
#   edits behind a real macro feature.
#
#   or a table generated from the TSV, checked in beside
#   `class_surface.pregen.rs`. Mechanical, but it puts ~2,300 rows of ruby's
#   answers into every binary -- roughly 150KB for a reflection surface almost
#   nothing reads -- and leaves two sources of truth to drift.
#
# Neither is a fix to squeeze in beside another one; recorded here with the
# numbers so the next pass can pick without re-measuring.
p method(:require).parameters
p String.instance_method(:sub).parameters
