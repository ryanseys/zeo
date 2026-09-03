# `getch` and `getpass` over a real pty -- the end-to-end shape `irb` and
# `reline` depend on.
#
# Both HUNG. The mode setter used `TCSADRAIN`, which waits for the terminal's
# pending output to be transmitted, so the switch into raw mode blocked
# forever whenever nobody was draining the other end. io-console uses
# `TCSANOW` with a retry on EINTR.

require "io/console"
require "pty"

m, s = PTY.open
p s.tty?

m.write("A")
sleep 0.05
p s.getch

m.write("BC")
sleep 0.05
p [s.getch, s.getch]

# The mode is restored around the scoped forms, whatever the block does.
p s.echo?
p(s.raw { s.echo? })
p s.echo?
p(s.noecho { s.echo? })
p s.echo?

begin
  s.raw { raise "boom" }
rescue RuntimeError => e
  p [e.message, s.echo?]
end

m.write("pw\r")
sleep 0.05
p s.getpass("")
p s.echo?

# `console_mode` is a detached copy, and `#raw` on it answers a NEW mode
# rather than destroying the saved one.
saved = s.console_mode
p saved.class
p saved.raw.equal?(saved)
p saved.raw.class
restored = (s.console_mode = saved)
p restored.class

p s.winsize = [24, 80]
p s.winsize
p s.ttyname.frozen?

m.close
s.close
__END__
true
"A"
["B", "C"]
true
false
true
false
true
["boom", true]
"pw"
true
IO::Console::Mode
false
IO::Console::Mode
IO::Console::Mode
[24, 80]
[24, 80]
true
