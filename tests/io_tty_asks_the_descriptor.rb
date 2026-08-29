# `IO#tty?` is `isatty(2)` on the handle's descriptor, not a test of which
# standard stream this is.
#
# Reading only the three std streams called EVERY other stream a file: a pty
# a gem opened answered false, which is exactly the word `io/console` and
# `reline` read before they go raw.

require "io/console"
require "pty"

m, s = PTY.open
p [s.tty?, s.isatty, m.tty?]

r, w = IO.pipe
p [r.tty?, w.tty?]

f = File.open("/dev/null")
p f.tty?
f.close

# A closed stream refuses BEFORE the syscall (`GetOpenFile`), rather than
# asking about descriptor 0.
begin
  f.tty?
rescue IOError => e
  p [e.class, e.message]
end

r.close
w.close
m.close
s.close
