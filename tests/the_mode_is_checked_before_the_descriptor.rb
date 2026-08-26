# CRuby asks whether a handle is OPEN for the direction before it touches
# the descriptor, so a read of a write-only handle is an `IOError` naming
# the mode -- never the kernel's `EBADF`, and never a made-up answer.
#
# Zeo asked at three rows and not at the other seventeen. Four of the
# unguarded ones did not even raise: `getc` answered a NUL character,
# `getbyte` answered 0, `readchar` and `readbyte` answered the same, and
# `eof?` answered `true`. A byte that is not there, reported as data.
#
# The guard sits in the read FUNNEL (`with_buffered_file`) rather than at
# each row, plus the few rows that reach the descriptor another way. That
# is the difference between fixing seventeen rows and stopping the
# eighteenth being written.
#
# `each_byte` is NOT here: it is a decided divergence, and
# `tests/each_byte_on_a_write_only_handle.rb` records it.

require "tmpdir"

def show(name)
  r = yield
  puts "#{name}\t#{r.inspect}"
rescue Exception => e
  puts "#{name}\t#{e.class}: #{e.message.sub(/ - .*/, '')}"
end

Dir.mktmpdir do |dir|
  path = File.join(dir, "f")
  File.write(path, "abcd\nefgh\n")

  # A WRITE-only handle refuses every read.
  File.open(path, "w") do |f|
    %w[read gets getc getbyte readchar readbyte readline readlines
       readpartial sysread read_nonblock each_line each_char
       each_codepoint eof?].each do |m|
      show(m) do
        case m
        when "readpartial", "sysread", "read_nonblock" then f.send(m, 2)
        when /^each/ then f.send(m) { |_| }
        else f.send(m)
        end
      end
    end
  end

  # A READ-only handle refuses every write.
  File.write(path, "abcd\n")
  File.open(path, "r") do |f|
    %w[write print << puts printf putc syswrite write_nonblock
       truncate].each do |m|
      show(m) do
        case m
        when "printf" then f.printf("%s", "x")
        when "putc" then f.putc(65)
        when "truncate" then f.truncate(1)
        else f.send(m, "x")
        end
      end
    end
  end

  # The refusals changed nothing on disk.
  show("content") { File.read(path) }

  # A READ-WRITE handle refuses neither.
  File.open(path, "r+") do |f|
    show("rw read") { f.read }
    show("rw write") { f.write("z") }
    show("rw eof?") { f.eof? }
  end
end
