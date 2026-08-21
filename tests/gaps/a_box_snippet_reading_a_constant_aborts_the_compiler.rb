# A run-time `box.eval` whose source reads a bare constant ABORTS the
# process: the emitter asserts that every box has a registered surrogate,
# and a SNIPPET compile has no boxes at all.
#
# `clif/expr.rs`'s `box_top()` does
# `box_surrogate(fx.box_id).expect("analyze registers a surrogate for every
# allocated box")`. That holds for a whole-program compile, where analyze
# mints one surrogate per `Ruby::Box.new` it sees. A snippet is compiled on
# its own -- `hir.boxes == 0` -- while `fx.box_id` is still the box the eval
# runs in, so the lookup misses and the `expect` fires. It has three
# reachable callers (`clif/expr.rs` twice, `clif/stmt.rs` once), so any
# top-level constant READ, WRITE or `defined?` in a box snippet takes it.
#
# A panic cannot unwind out of `extern "C"`, so this is an abort rather than
# an exception: `fatal runtime error: failed to initiate panic`.
#
# The LITERAL form is spliced at compile time and does not reach it, which is
# why this file computes the source string -- `["K", ""].first` is opaque to
# the constant folder, so the eval is compiled at run time.
#
# The fix is G7's B0: a snippet's `box_id` has to resolve through the
# `BoxTable` the run-time box model registers, not through the compile-time
# surrogate map. Until then this is the shape that takes the process down.
#
# Oracle: the box cannot see main's `K`, so ruby raises NameError from the
# snippet's own frame.
K = 5
b = Ruby::Box.new
src = ["K", ""].first
p b.eval(src)
