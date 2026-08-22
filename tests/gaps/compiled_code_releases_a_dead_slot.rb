# Compiled code hands `zeo_rt_release` a stack slot that holds no live value.
#
# `ZEO_RT_LEAKCHECK=1` is the compiled-ownership ledger, and it reports the
# tag byte it found: 176 here, 64 in `tests/tracepoint.rb`. Neither is a value
# tag (they run 0..33) and neither is the poison byte a released slot is
# stamped with (0xFF), so the slot is not a value that was freed -- it is a
# slot that never held one. The release then runs `drop_in_place` over
# whatever the stack had there.
#
# This is a MEMORY-SAFETY bug in the emitter's ownership lowering, not a
# divergence. It was found while arming the cycle collector and it is older
# than that work -- it reproduces at `7c5a2247`, before any of it.
#
# WHAT IT COSTS TODAY. Nothing visible, most of the time: the corpus is green
# and these programs print the right answers. It surfaced once as an
# intermittent abort inside an unrelated `Vec`'s drop, which is what a
# `drop_in_place` over a garbage slot looks like from a distance.
#
# THE FULL LIST, over `tests/*.rb`. Two shapes, five programs:
#
#   * a released slot that holds no live value -- `pp_pretty_print.rb`
#     (minimized below), `tracepoint.rb`, `rspec_end_to_end.rb`;
#   * a ledger IMBALANCE at exit, which is a leak rather than a bad release --
#     `unknown_qualified_superclass.rb` and `method_capture_before_own_def.rb`
#     both leak one `Proc` (tag 24). That one minimizes to:
#
#         module Rendering
#           Template = "shadows the leaf"
#         end
#         begin
#           class Tilt < ::Tilt::Template
#           end
#         rescue NameError => e
#           p e.class
#         end
#
#     and it needs the shadowing constant: without it the definition is
#     deferred rather than registered, and nothing leaks.
#
# WHY THE LEDGER NEVER SAID SO. It could not: `LIVE[t as usize]` indexed a
# 64-entry table with the byte it read, so a byte outside the tag range
# panicked inside the diagnostic with "index out of bounds" instead of
# naming the problem. That is fixed; these five are what it found the first
# time it could speak.
#
# `clif/verify.rs`'s per-site ledger is the other half of this and passes,
# so whatever produces the slot balances at the site it is emitted from.
#
# Oracle: the program prints its line and exits 0.
require "pp"

class Plain
end

PP.pp(Plain.new, +"")
puts "done"
