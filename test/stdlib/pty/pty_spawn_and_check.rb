# `PTY` against CRuby's `ext/pty/pty.c`.
#
# Five of these were wrong. `PTY.spawn` handed back plain `IO`s where ruby
# gives `File`s named for the slave device; its block form answered the
# block's value where ruby answers nil; with no command at all it raised
# instead of starting a login shell; a command it could not launch blamed the
# empty string rather than "fork failed"; and `PTY.check` on a pid that is not
# this process's child raised `Errno::ECHILD` where ruby answers nil, which is
# the whole point of a non-blocking poll.
#
# `PTY::ChildExited#status` also answered nil: the raise carried only a
# message, so the `Process::Status` never reached the exception.

require "pty"

m, s = PTY.open
p [m.class.to_s, s.class.to_s]
p [m.path.start_with?("masterpty:/dev/"), s.path.start_with?("/dev/")]
p m.path == "masterpty:#{s.path}"
p [m.tty?, s.tty?, m.fileno != s.fileno]
m.close
s.close

# The block form of `open` closes the pair after.
p(PTY.open { |mm, ss| [mm.class.to_s, ss.tty?] })

r, w, pid = PTY.spawn("echo", "hi")
p [r.class.to_s, w.class.to_s, pid.is_a?(Integer)]
p [r.path == w.path, r.path.start_with?("/dev/"), r.pid]
out = begin
        r.gets
      rescue Errno::EIO
        nil
      end
p out&.chomp
Process.wait(pid)
r.close
w.close

# The block form answers nil, not the block's value.
p(PTY.spawn("echo", "block") do |rr, _ww, cpid|
  begin
    rr.gets
  rescue Errno::EIO
  end
  Process.wait(cpid)
  :from_the_block
end)

# A command that cannot start blames "fork failed", whatever the errno.
begin
  PTY.spawn("this-command-does-not-exist-zeo")
rescue SystemCallError => e
  p [e.class.to_s, e.message]
end

# A pid that is not this process's child is nil, never an error.
p PTY.check(999_999)
p PTY.check(1)

r2, w2, pid2 = PTY.spawn("true")
sleep 0.3
p PTY.check(pid2).class.to_s
r2.close
w2.close

r3, w3, pid3 = PTY.spawn("true")
sleep 0.3
begin
  PTY.check(pid3, true)
rescue PTY::ChildExited => e
  p [e.class.to_s, e.message.start_with?("pty - exited: "), e.status.class.to_s,
     e.status.exitstatus]
end
r3.close
w3.close

p PTY.respond_to?(:getpty)
p PTY::ChildExited.superclass.to_s
__END__
["IO", "File"]
[true, true]
true
[true, true, true]
["IO", true]
["File", "File", true]
[true, true, nil]
"hi"
nil
["Errno::ENOENT", "No such file or directory - fork failed"]
nil
nil
"Process::Status"
["PTY::ChildExited", true, "Process::Status", 0]
true
"RuntimeError"
