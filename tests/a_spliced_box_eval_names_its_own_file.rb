# A literal `box.eval` is SPLICED at compile time, and the snippet is a
# file of its own -- `__FILE__` answers `eval`, the same name the runtime
# `eval` entry gives a computed snippet. A backtrace must not say which
# tier ran the code.
#
# What is still the caller's is the FRAME: a spliced body runs inside the
# enclosing method's frame, and a frame carries ONE file string, stamped
# once at emit time, while only the line is stamped per statement. So the
# raise below reports the snippet's line under the caller's file and
# label. Giving it its own frame costs two frame pushes per execution on
# the path whose whole justification is being zero-cost, so it stays
# recorded in `tests/gaps/a_box_literal_eval_is_spliced.rb`.
b = Ruby::Box.new
p b.eval("__FILE__")
p b.eval("__FILE__ + '/x'")
# Two statements: the second names the same file and its own line.
p b.eval("__FILE__\n__LINE__")
p b.eval("[__FILE__, __LINE__]")
# The caller's own `__FILE__` is untouched by the splice beside it.
p File.basename(__FILE__)
