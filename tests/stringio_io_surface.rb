require "stringio"

# getc reads one character at a time (UTF-8 aware); nil at EOF.
io = StringIO.new("hé!")
p io.getc
p io.getc
p io.getc
p io.getc

# seek repositions by SEEK_SET (0), SEEK_CUR (1), or SEEK_END (2).
buf = StringIO.new("abcdef")
buf.seek(2)
p buf.read(2)
buf.seek(-1, IO::SEEK_END)
p buf.read
buf.seek(1, IO::SEEK_SET)
buf.seek(1, IO::SEEK_CUR)
p buf.read(1)

# readline raises EOFError past the end; readlines gathers the rest.
lines = StringIO.new("one\ntwo\nthree")
p lines.readline
p lines.readlines
begin
  lines.readline
rescue EOFError => e
  puts "EOFError: #{e.message}"
end

# truncate resizes the buffer.
t = StringIO.new("hello world")
t.truncate(5)
p t.string
