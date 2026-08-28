# `readpartial`, `sysread`, `read(n, buf)` and `pread` answer BYTES.
#
# Each of them used to route the bytes through `String::from_utf8_lossy`,
# which replaces every byte no character claims with a three-byte U+FFFD --
# so a 100-byte read answered 136 bytes of something else, and the answer was
# tagged UTF-8 rather than ASCII-8BIT.
#
# rubygems digests each member of a `.gem` through `readpartial(16384, buf)`,
# so every downloaded gem failed its SHA256 checksum and nothing could be
# installed. Nothing else caught it because a text file has no such bytes.

require "digest"

PATH = File.expand_path("fixtures/every_byte.bin", __dir__)

def show(label, s)
  puts "#{label}\tbytesize=#{s.bytesize}\tsize=#{s.size}\tenc=#{s.encoding}\t#{Digest::SHA256.hexdigest(s)[0, 16]}"
end

File.open(PATH, "rb") { |io| show "read(n)", io.read(200) }
File.open(PATH, "rb") { |io| show "readpartial(n)", io.readpartial(200) }
File.open(PATH, "rb") { |io| show "sysread(n)", io.sysread(200) }
File.open(PATH, "rb") { |io| show "pread", io.pread(200, 0) }

# The output-buffer forms fill the caller's own String and answer it.
File.open(PATH, "rb") do |io|
  buf = String.new(capacity: 16_384, encoding: Encoding::BINARY)
  r = io.readpartial(200, buf)
  show "readpartial(n, buf)", buf
  puts "same object\t#{r.equal?(buf)}"
end

File.open(PATH, "rb") do |io|
  buf = +""
  io.read(200, buf)
  show "read(n, buf)", buf
end

File.open(PATH, "rb") do |io|
  buf = +""
  io.pread(200, 0, buf)
  show "pread(n, off, buf)", buf
end

# A short read stops at the end rather than over-reading.
File.open(PATH, "rb") { |io| show "read past end", io.read(10_000) }

# Reading the whole file in chunks reproduces it exactly, which is the
# property rubygems' checksum depends on.
digest = Digest::SHA256.new
File.open(PATH, "rb") do |io|
  buf = String.new(capacity: 64, encoding: Encoding::BINARY)
  until io.eof?
    io.readpartial(64, buf)
    digest << buf
  end
end
puts "chunked\t#{digest.hexdigest == Digest::SHA256.file(PATH).hexdigest}"
