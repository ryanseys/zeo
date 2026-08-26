# A `box.eval` with a LITERAL source is spliced into the program at compile
# time -- the fast, statically-typed path a box program is written for -- so
# its BACKTRACE reports the program's frame where CRuby reports the
# snippet's.
#
# The computed form is right (see
# `tests/a_box_snippet_names_itself_in_a_backtrace.rb`): file `eval`, scope
# `<compiled>`, entry frame `Ruby::Box#eval`.
#
# HALF of this is fixed. The splice registers the snippet as a file of its
# own, so `__FILE__` and `__LINE__` answer `eval` and the snippet's own
# line -- `tests/a_spliced_box_eval_names_its_own_file.rb` holds that half.
#
# What remains is the FRAME, and it is not the same size. A frame carries
# ONE file string, stamped once at emit time; only the line is stamped per
# statement. So a spliced body cannot name its own file in a backtrace
# without a frame of its own -- two frame pushes per execution, on the path
# whose entire justification is being zero-cost, to fix one backtrace row
# in a construct nobody puts in a loop. It waits for a pass of its own.
#
# Ruby's answer: a literal box eval names itself exactly as a computed one
# does.
b = Ruby::Box.new
p b.eval("__FILE__")
begin
  b.eval("raise 'x'")
rescue RuntimeError => e
  p e.backtrace.first(2)
end
