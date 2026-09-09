require "tmpdir"
ZTMP = Dir.mktmpdir

r, w = IO.pipe
w.write("hello\0world")
w.close
data = r.read(100)
puts "got " + data.length.to_s + " bytes"
p data.bytes.length
p data[5].ord
r.close

path = File.join(ZTMP, "sp_nul_file_#{Process.pid}")
File.write(path, "a\0b\0c")
s = File.read(path)
p s.length
p s.bytes
File.delete(path)
__END__
got 11 bytes
11
0
5
[97, 0, 98, 0, 99]
