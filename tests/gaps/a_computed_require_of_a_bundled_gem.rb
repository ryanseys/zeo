# A computed `require` of a BUNDLED GEM finds nothing, and that is a
# DECISION rather than a missing mechanism.
#
# `x = ["json", ""].first; require x` raises `LoadError: cannot load such
# file -- json`, where the same require spelled literally splices json in
# and works.
#
# The two automatic tiers both miss it. The eager unit sweep compiles in the
# files on a COMPILE-TIME load path, and a bundled gem is not on one -- it
# lives inside zeo's own tree, reached by the loader's gem resolution rather
# than by `-I`. The run-time tier searches the program's `$LOAD_PATH` on
# disk, and a compiled binary's carries no path to a gem that was never
# installed on the machine it runs on.
#
# The third tier now EXISTS: `--embed-sources <dir>` carries a directory's
# `.rb` files inside the program, and the run-time loader resolves against
# them before it looks at disk (`crates/zeo/src/embed.rs`,
# `features::resolve_embedded`). It works for main and for a box, in the JIT
# and in a linked binary -- `e2e/gems_require.rs` pins all four.
#
# What it is NOT is automatic, and it cannot be: a computed target is opaque,
# so "embed what it might name" means embedding every gem the program can
# see. A hermetic binary is the choice zeo makes, and doubling every artifact
# for a tier most programs never reach is not a default. Making THIS file
# pass means `zeo --embed-sources <gems>/json/lib`, and a golden takes no
# compiler flags.
#
# Its neighbours say how narrow the automatic half is: a computed require of
# a file under a compile-time `-I` root works (the unit sweep), and one on
# the run-time `$LOAD_PATH` works (the disk tier,
# `a_file_the_compiler_never_saw_loads_at_run_time.rb`).
#
# Oracle: the gem loads.
name = ["json", ""].first
require name
p JSON.generate([1, 2])
