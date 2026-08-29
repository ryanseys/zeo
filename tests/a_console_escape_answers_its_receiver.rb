# The cursor/erase/scroll family writes an escape and answers the RECEIVER,
# so `io.goto(0, 0).erase_line(0)` chains. They answered nil.
#
# Two rules ride along, both from `console_move`/`console_scroll`: a distance
# of ZERO writes nothing at all, and a NEGATIVE one flips the direction
# rather than emitting a negative number -- `cursor_up(-3)` moves three DOWN.
# The old `format!` wrote `ESC[0A` and `ESC[-3A`, the second of which is not
# an escape sequence.

require "stringio"
require "io/console"

out = StringIO.new
p out.respond_to?(:goto)

r, w = IO.pipe

def bytes(w, r)
  w.flush
  r.read_nonblock(400)
rescue IO::WaitReadable, EOFError
  ""
end

p w.goto(3, 5).equal?(w)
p bytes(w, r)

p w.goto_column(7).equal?(w)
p bytes(w, r)

p w.cursor_up(2).equal?(w)
p w.cursor_down(2).equal?(w)
p w.cursor_left(2).equal?(w)
p w.cursor_right(2).equal?(w)
p bytes(w, r)

# Zero writes nothing.
p w.cursor_up(0).equal?(w)
p w.scroll_forward(0).equal?(w)
p bytes(w, r)

# A negative distance flips the letter and drops the sign.
p w.cursor_up(-3).equal?(w)
p w.cursor_left(-4).equal?(w)
p w.scroll_forward(-2).equal?(w)
p w.scroll_backward(-2).equal?(w)
p bytes(w, r)

p w.erase_line(0).equal?(w)
p w.erase_screen(2).equal?(w)
p w.clear_screen.equal?(w)
p bytes(w, r)

p w.cursor = [3, 5]
p bytes(w, r)

r.close
w.close
