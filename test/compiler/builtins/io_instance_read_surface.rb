# IO instance reads: gets separator/limit/chomp, readline EOFError, lineno,
# getc/readchar/getbyte, each_line separator + fresh strings, printf/putc,
# pos=, readpartial, flock, each_char/each_byte, multi-arg write, sysseek.
require "tmpdir"
ZTMP = Dir.mktmpdir

File.write(File.join(ZTMP, "sp_io3.txt"), "hello\nworld\n")
File.open(File.join(ZTMP, "sp_io3.txt")) { |f| p f.gets("o") }
File.open(File.join(ZTMP, "sp_io3.txt")) { |f| p f.gets(3) }
File.open(File.join(ZTMP, "sp_io3.txt")) { |f| p f.gets(chomp: true) }
File.open(File.join(ZTMP, "sp_io3.txt")) do |f|
  f.read
  r = (f.readline rescue $!.class)
  p r
end
File.open(File.join(ZTMP, "sp_io3.txt")) { |f| f.gets; p f.lineno }
File.open(File.join(ZTMP, "sp_io3.txt")) { |f| p f.getc }
File.open(File.join(ZTMP, "sp_io3.txt")) { |f| p f.readchar }
File.open(File.join(ZTMP, "sp_io3.txt")) { |f| p f.getbyte }
File.open(File.join(ZTMP, "sp_io3.txt")) { |f| f.each_line("o") { |l| p l } }
File.write(File.join(ZTMP, "sp_io3b.txt"), "one\ntwo\nthree\n")
File.open(File.join(ZTMP, "sp_io3b.txt")) do |f|
  lines = []
  f.each_line { |l| lines << l }
  p lines
end
File.open(File.join(ZTMP, "sp_io3.txt"), "w") { |f| f.printf("%03d-%s", 7, "x") }
p File.read(File.join(ZTMP, "sp_io3.txt"))
File.open(File.join(ZTMP, "sp_io3.txt"), "w") { |f| p f.putc(65); f.putc("bc") }
p File.read(File.join(ZTMP, "sp_io3.txt"))
File.write(File.join(ZTMP, "sp_io3.txt"), "hello")
File.open(File.join(ZTMP, "sp_io3.txt")) { |f| f.pos = 2; p f.read }
File.open(File.join(ZTMP, "sp_io3.txt")) { |f| p f.readpartial(3) }
File.open(File.join(ZTMP, "sp_io3.txt")) { |f| p f.flock(File::LOCK_EX) }
File.open(File.join(ZTMP, "sp_io3.txt")) { |f| cs = []; f.each_char { |ch| cs << ch }; p cs }
File.open(File.join(ZTMP, "sp_io3.txt")) { |f| bs = []; f.each_byte { |bb| bs << bb }; p bs }
File.open(File.join(ZTMP, "sp_io3.txt"), "w") { |f| p f.write("a", "b", "c") }
p File.read(File.join(ZTMP, "sp_io3.txt"))
File.open(File.join(ZTMP, "sp_io3.txt")) { |f| p f.sysseek(2) }
File.delete(File.join(ZTMP, "sp_io3.txt"), File.join(ZTMP, "sp_io3b.txt"))
__END__
"hello"
"hel"
"hello"
EOFError
1
"h"
"h"
104
"hello"
"\nwo"
"rld\n"
["one\n", "two\n", "three\n"]
"007-x"
65
"Ab"
"llo"
"hel"
0
["h", "e", "l", "l", "o"]
[104, 101, 108, 108, 111]
3
"abc"
2
