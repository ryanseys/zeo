# Line and character reads buffer ahead, so every operation that observes or
# moves the file position has to see the descriptor where RUBY consumed to,
# never where the read-ahead left it.

path = "/tmp/zeo_buffered_read_#{Process.pid}.txt"
File.open(path, "w") { |f| f.write("alpha\nbeta\ngamma\ndelta\n") }

# `pos` after a buffered line read.
File.open(path, "r") do |f|
  p f.gets
  p f.pos
  p f.gets
  p f.pos
end

# `read` picks up exactly where `gets` stopped.
File.open(path, "r") do |f|
  p f.gets
  p f.read
end

# `seek` and `rewind` land where they say, mid-buffer.
File.open(path, "r") do |f|
  f.gets
  f.seek(6)
  p f.gets
  f.rewind
  p f.gets
  f.seek(-6, IO::SEEK_END)
  p f.read
end

# `getc` and `getbyte` share the same buffer as `gets`.
File.open(path, "r") do |f|
  p f.getc
  p f.getbyte
  p f.gets
  p f.pos
end

# `ungetbyte` is still returned ahead of anything buffered.
File.open(path, "r") do |f|
  f.gets
  f.ungetbyte(88)
  p f.getbyte
  p f.gets
end

# `eof?` reports on the logical position, not the read-ahead one.
File.open(path, "r") do |f|
  p f.eof?
  f.gets
  p f.eof?
  f.read
  p f.eof?
end

# A separate handle sees the file whole, whatever the first one buffered.
File.open(path, "r") do |f|
  f.gets
  File.open(path, "r") { |g| p g.read.length }
  p f.pos
end

# `each_line` leaves the position at end of file.
File.open(path, "r") do |f|
  n = 0
  f.each_line { |l| n += 1 }
  p n
  p f.pos
  p f.eof?
end

# Breaking out of `each_line` stops after the line the block last saw.
File.open(path, "r") do |f|
  f.each_line do |l|
    break if l.start_with?("beta")
  end
  p f.pos
  p f.gets
end

File.delete(path)
