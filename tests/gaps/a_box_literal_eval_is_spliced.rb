# A `box.eval` with a LITERAL source is spliced into the program at compile
# time -- the fast, statically-typed path a box program is written for -- so
# it reports the PROGRAM's file and frame where CRuby reports the snippet's.
#
# The computed form is right (see
# `tests/a_box_snippet_names_itself_in_a_backtrace.rb`): file `eval`, scope
# `<compiled>`, entry frame `Ruby::Box#eval`. The splice has no snippet to
# name, which is the whole point of it.
#
# The fix shape: the splice registers its source as a file of its own
# (`Hir::add_file("eval", &src)`, which the FFI-struct lowering already does)
# and lifts the body to a scope labelled `<compiled>`. That is a real change
# to a fast path, so it waits for a pass of its own.
#
# Oracle: a literal box eval names itself exactly as a computed one does.
b = Ruby::Box.new
p b.eval("__FILE__")
begin
  b.eval("raise 'x'")
rescue RuntimeError => e
  p e.backtrace.first(2)
end
