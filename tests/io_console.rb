# `io/console` -- terminal modes, echo control, and the cursor escapes. CRuby
# ships it as a require-gated extension; zeo's rows hang unconditionally off
# the IO table, so the require is ceremony (`docs/EXTENSIONS.md`).
#
# Nothing here is a terminal, which is the only path a golden can assert
# without a pty. The streams are a pipe and a regular file rather than STDIN:
# the harness gives STDIN /dev/null, and tcgetattr on a character device
# answers ENODEV on some platforms and ENOTTY on others.
require "io/console"

p STDIN.tty?
p IO.console
p STDIN.ttyname

def refusal(label)
  yield
  "#{label}: no error"
rescue SystemCallError, NotImplementedError => e
  "#{label}: #{e.class}: #{e.message}"
end

# A stream with no name reports none; the direct methods on a named one name
# it, and the scoped forms never do -- CRuby reaches that failure through a
# helper with no name to report.
r, w = IO.pipe
puts refusal("pipe raw!") { r.raw! }
puts refusal("pipe raw") { r.raw {} }
puts refusal("pipe winsize") { r.winsize }

f = File.open("io_console_probe.txt", "w+")
puts refusal("file raw!") { f.raw! }
puts refusal("file raw") { f.raw {} }
puts refusal("file cooked!") { f.cooked! }
puts refusal("file cooked") { f.cooked {} }
puts refusal("file echo?") { f.echo? }
puts refusal("file echo=") { f.echo = false }
puts refusal("file noecho") { f.noecho {} }
puts refusal("file getch") { f.getch }
puts refusal("file getpass") { f.getpass }
puts refusal("file iflush") { f.iflush }
puts refusal("file oflush") { f.oflush }
puts refusal("file ioflush") { f.ioflush }
puts refusal("file winsize") { f.winsize }
puts refusal("file winsize=") { f.winsize = [10, 20] }
puts refusal("file console_mode") { f.console_mode }
# Windows-only in CRuby as well -- the message is its own.
puts refusal("file pressed?") { f.pressed? }
puts refusal("file check_winsize_changed") { f.check_winsize_changed }
p f.ttyname
f.close
File.unlink("io_console_probe.txt")

# The cursor and erase escapes only WRITE, so they work on any stream -- which
# is why redirecting a program's output captures them rather than failing.
w.goto(2, 3)
w.goto_column(5)
w.cursor_up(1)
w.cursor_down(2)
w.cursor_left(3)
w.cursor_right(4)
w.scroll_forward(5)
w.scroll_backward(6)
w.erase_line(0)
w.erase_screen(2)
w.clear_screen
w.beep
w.close
p r.read
r.close
