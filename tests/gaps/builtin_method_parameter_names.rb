# `Method#parameters` on a BUILTIN row answers the anonymous descriptor
# derived from its arity -- `[[:req]]` where ruby says `[[:req, :path]]`.
# zeo reports a parameter NAME on no builtin row at all.
#
# RE-MEASURED 2026-08-21, live against the oracle rather than against the
# arity TSV -- whose 8th column carries ruby's KINDS, not its names, which is
# what made the earlier count wrong. Diffing `#parameters` over every class
# reachable from `Object` (5,444 ruby rows, 2,367 of them shared with zeo):
# **138 rows diverge**, clustered as
#
#   Pathname 44 · Ractor 16 · GC 8 · Array 7 · Thread::SizedQueue 6 ·
#   Random::Formatter 5 · RubyVM::YJIT 5 · Kernel 5 ·
#   RubyVM::AbstractSyntaxTree 4 · Process::Tms 4 · ObjectSpace 4 ·
#   Exception 4 · TracePoint 3 · Thread::Queue 3 · IO 3 · Dir 3 ·
#   then a tail of one- and two-row classes.
#
# The earlier reading (265 rows; ObjectSpace 50, IO 40, RubyVM::YJIT 16) was
# counting differently and is superseded. It matters because it was the basis
# for scoping: G8's corelib closes Pathname (44) and Kernel (5) by
# construction, so ~89 rows are NOT reachable that way -- GC, Array, the
# queue family, Random::Formatter, Process::Tms, Exception, TracePoint, Dir --
# and those are the ones worth hand-annotating.
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
