# IO class methods (read/write/readlines/foreach/pipe/copy_stream/sysopen),
# Kernel#open, read-into-buffer, puts with an Array, readlines separators.
require "tmpdir"
ZTMP = Dir.mktmpdir

File.write(File.join(ZTMP, "sp_io4.txt"), "one\ntwo\n")
p IO.read(File.join(ZTMP, "sp_io4.txt"))
p IO.readlines(File.join(ZTMP, "sp_io4.txt"))
IO.write(File.join(ZTMP, "sp_io4.txt"), "abc")
p IO.read(File.join(ZTMP, "sp_io4.txt"))
IO.foreach(File.join(ZTMP, "sp_io4.txt")) { |l| p l }
rw = IO.pipe
p rw.class
w = rw[1]
r = rw[0]
w.write("ping")
w.close
p r.read
r.close
File.write(File.join(ZTMP, "sp_io4.txt"), "hello")
p IO.copy_stream(File.join(ZTMP, "sp_io4.txt"), File.join(ZTMP, "sp_io4b.txt"))
p File.read(File.join(ZTMP, "sp_io4b.txt"))
fd = IO.sysopen(File.join(ZTMP, "sp_io4.txt"))
p fd.class
open(File.join(ZTMP, "sp_io4.txt")) { |f| p f.read }
File.open(File.join(ZTMP, "sp_io4.txt")) do |f|
  b001 = ""
  f.read(5, b001)
  p b001
end
File.open(File.join(ZTMP, "sp_io4.txt"), "w") { |f| f.puts([1, 2, 3]) }
p File.read(File.join(ZTMP, "sp_io4.txt"))
File.write(File.join(ZTMP, "sp_io4.txt"), "foo\nbar\n")
p File.readlines(File.join(ZTMP, "sp_io4.txt"), "o")
File.open(File.join(ZTMP, "sp_io4.txt")) { |f| p f.readlines("o") }
File.open(File.join(ZTMP, "sp_io4.txt")) { |f| p f.readlines(chomp: true) }
File.delete(File.join(ZTMP, "sp_io4.txt"), File.join(ZTMP, "sp_io4b.txt"))
__END__
"one\ntwo\n"
["one\n", "two\n"]
"abc"
"abc"
Array
"ping"
5
"hello"
Integer
"hello"
"hello"
"1\n2\n3\n"
["fo", "o", "\nbar\n"]
["fo", "o", "\nbar\n"]
["foo", "bar"]
