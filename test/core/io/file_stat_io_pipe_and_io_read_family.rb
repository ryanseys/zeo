# File::Stat (class + readers), File.stat/File#stat, IO.pipe
# (a plain-IO reader/writer pair), the IO instance read family
# (gets separator/limit/chomp, getc/getbyte, lineno), IO class methods
# (IO.read/write/copy_stream), and FileTest.
require "tmpdir"
ZTMP = Dir.mktmpdir


path = File.join(ZTMP, "sp_e2e_stat_#{Process.pid}.txt")
File.write(path, "hello\nworld\n")
st = File.stat(path)
p st.class
p st.size
p st.file?
p st.directory?
File.open(path) { |f| p f.stat.class }
File.open(path) { |f| p f.gets("o") }
File.open(path) { |f| p f.gets(3) }
File.open(path) { |f| p f.gets(chomp: true); p f.lineno }
File.open(path) { |f| p f.getc; p f.getbyte }
p FileTest.file?(path)
p FileTest.directory?(path)
r, w = IO.pipe
p r.class
w.write("ping")
w.close
p r.read
r.close
p IO.read(path, 5)
File.delete(path)
__END__
File::Stat
12
true
false
File::Stat
"hello"
"hel"
"hello"
1
"h"
101
true
false
IO
"ping"
"hello"
