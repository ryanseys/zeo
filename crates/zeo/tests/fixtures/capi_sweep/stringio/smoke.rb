require "stringio"

io = StringIO.new("a\nb\nc")
p io.gets, io.read, io.eof?, io.pos
io = StringIO.new
io.puts "x"
io.write 1, 2.5
io.print "z"
p io.string, io.size
io.rewind
p io.each_line.to_a, io.getc
io = StringIO.new("héllo")
p io.read(2), io.read, io.read
p StringIO.new("abc").each_char.to_a, StringIO.new("q").readlines
