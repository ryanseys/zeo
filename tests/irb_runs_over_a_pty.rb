# irb, driven the way a person drives it: a real pty, typed lines, read back.
#
# This is the end-to-end proof of the io/console work. irb reaches for
# `IO.console`, `#getch`, `#winsize` and the cursor escapes on its way up, and
# reline sits on top of all of them -- so a shell that answers `=> 3` has
# exercised the whole surface rather than a method list.
#
# It drives `RbConfig.ruby`, so each runtime starts its OWN irb: ruby's under
# ruby, zeo's under zeo. Only the result lines are kept, because the banner
# names the user and the terminal negotiation differs run to run.

require "pty"
require "rbconfig"

out = +""
PTY.spawn({ "TERM" => "dumb" }, RbConfig.ruby, "-e", 'require "irb"; IRB.start') do |r, w, pid|
  w.write("1 + 2\n")
  w.write("IO.console.class\n")
  w.write("STDIN.respond_to?(:getch)\n")
  w.write("exit\n")
  deadline = Time.now + 60
  begin
    while Time.now < deadline
      chunk = r.read_nonblock(4096, exception: false)
      if chunk == :wait_readable
        r.wait_readable(0.5)
      elsif chunk.nil?
        break
      else
        out << chunk
      end
    end
  rescue Errno::EIO, EOFError
    # The child closed its end; that is how a pty reports EOF.
  end
  begin
    Process.wait(pid)
  rescue Errno::ECHILD
  end
end

puts out.gsub(/\e\[[0-9;?]*[a-zA-Z]/, "").delete("\r").lines.grep(/=> /).map(&:strip)
