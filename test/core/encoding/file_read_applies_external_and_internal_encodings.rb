# File.read tags bytes with the external encoding (default UTF-8), or a
# requested one; binread is always ASCII-8BIT; binwrite round-trips raw
# bytes. Verified against ruby 4.0.6.

require "tmpdir"
Dir.mktmpdir do |dir|
  path = File.join(dir, "f.txt")
  File.write(path, "café")
  p File.read(path).encoding
  p File.read(path).bytesize
  p File.binread(path).encoding
  p File.binread(path).bytes
  p File.read(path, encoding: "ISO-8859-1").encoding
  p File.read(path, encoding: "ISO-8859-1").bytes
  File.binwrite(path, [0, 255, 128].pack("C*"))
  p File.binread(path).bytes
end
__END__
#<Encoding:UTF-8>
5
#<Encoding:BINARY (ASCII-8BIT)>
[99, 97, 102, 195, 169]
#<Encoding:ISO-8859-1>
[99, 97, 102, 195, 169]
[0, 255, 128]
