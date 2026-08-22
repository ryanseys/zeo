# A feature zeo satisfies natively (`io/console`, `set`, `pathname`, ...) is
# recorded for the WHOLE program at startup, not at the `require`'s own
# document position. So a program that requires `io/console` on its last
# line already answers `respond_to?(:getch)` on its first.
#
# This is the shape [[zeo-positional-runtime-flags]] records: a directive the
# RUN TIME owns, folded into a compile-time set and applied before line 1.
# The never-required case is right (see
# `tests/a_require_gated_row_waits_for_its_require.rb`); it is the ordering
# inside one program that is not.
#
# Two mechanisms carry the divergence and both are whole-program:
# `Hir::activate_feature` (a compile-time set the emitter reads) and the
# `$LOADED_FEATURES` seed, which `builtins::gate` asks. The fix shape is a
# run-time marker emitted at the require's own site --
# `activate_static_ext` returns `Ok(Vec::new())` today and would return one
# statement -- with the gate reading that instead of `$LOADED_FEATURES`.
#
# Oracle: the rows appear at the require, not before it.
p STDOUT.respond_to?(:winsize)
p IO.instance_methods(false).include?(:getch)
require "io/console"
p STDOUT.respond_to?(:winsize)
p IO.instance_methods(false).include?(:getch)
