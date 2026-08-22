# A computed `require` of a BUNDLED GEM finds nothing.
#
# `x = ["json", ""].first; require x` raises `LoadError: cannot load such
# file -- json`, where the same require spelled literally splices json in
# and works.
#
# The two tiers a computed require has both miss it. The eager unit sweep
# compiles in the files on a COMPILE-TIME load path, and a bundled gem is
# not on one -- it lives inside zeo's own tree, reached by the loader's
# gem resolution rather than by `-I`. The run-time tier searches the
# program's `$LOAD_PATH` on disk, and a compiled binary's carries no path
# to a gem that was never installed on the machine it runs on.
#
# So this is the tier the plan calls `--embed-sources`: the compiler
# records the gem's own source in the program, and the run-time loader
# resolves against it before it looks at disk. The narrow version -- embed
# the STATIC CLOSURE of every gem a computed require could name -- is what
# makes this shape work without embedding a program's whole dependency
# tree by default.
#
# Its neighbours say how narrow it is: a computed require of a file under
# a compile-time `-I` root works (the unit sweep), a computed require of a
# file on the run-time `$LOAD_PATH` works (the disk tier,
# `a_file_the_compiler_never_saw_loads_at_run_time.rb`), and the literal
# spelling of this very require works.
#
# Oracle: the gem loads.
name = ["json", ""].first
require name
p JSON.generate([1, 2])
