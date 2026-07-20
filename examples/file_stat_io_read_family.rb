# File::Stat, IO.pipe, the IO instance read family, IO class methods, and
# FileTest -- the file/IO surface. Output is path-independent so the golden is
# stable.
path = "/tmp/sp_ex_stat_#{Process.pid}.txt"
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
File.open(path) { |f| p f.readlines }
File.open(path) { |f| f.pos = 6; p f.read }

p FileTest.file?(path)
p FileTest.directory?(path)

r, w = IO.pipe
p r.class
w.write("ping")
w.close
p r.read
r.close

p IO.read(path, 5)
p IO.readlines(path)
File.delete(path)
puts "done"
