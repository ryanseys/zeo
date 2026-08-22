# A box's snippet names itself differently from every other eval, in three
# places at once: the file is `eval` (not `(eval at f.rb:14)`), the snippet's
# own scope is `<compiled>` (not the caller's), and the frame between them is
# `Ruby::Box#eval` (not `Kernel#eval`).
#
# This file was filed because the same shape ABORTED the process -- the
# emitter asserted that every box has a compile-time surrogate and a SNIPPET
# compile has no boxes at all, and a panic cannot unwind out of `extern "C"`.
# G7's B0 fixed that; what was left was the naming, and a computed
# `box.eval` reaching `Kernel#eval` rather than the box's own row.
#
# The LITERAL form is spliced at compile time and still reports the
# program's own file -- see `tests/gaps/a_box_literal_eval_is_spliced.rb`.
# This file computes the source (`["K", ""].first` is opaque to the constant
# folder) so the eval is compiled at run time.
#
# Oracle: the box cannot see main's `K`, so ruby raises NameError from the
# snippet's own frame.
K = 5
b = Ruby::Box.new
src = ["K", ""].first
p b.eval(src)
