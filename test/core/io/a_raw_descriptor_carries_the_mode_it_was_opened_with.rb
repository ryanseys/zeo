# `IO.sysopen(path, mode, perm)` opens and hands back the raw fd. zeo read the
# path and dropped everything after it, so the descriptor always came back
# read-only and `IO.new(fd, "w").write` raised EBADF on a file the program had
# just asked to open for writing. `File.open` dropped `perm` the same way.
require "tmpdir"
require "fileutils"

dir = Dir.mktmpdir("zeo_sysopen")

a = File.join(dir, "a.txt")
File.open(a, "w", 0o600) { |f| f.write "x" }
p format("%o", File.stat(a).mode & 0o777)

b = File.join(dir, "b.txt")
fd = IO.sysopen(b, "w", 0o640)
w = IO.new(fd, "w")
w.write("written")
w.close
p format("%o", File.stat(b).mode & 0o777)
p File.read(b)

# An `O_*` bitmask says the same thing a mode string does.
c = File.join(dir, "c.txt")
File.write(c, "hello")
rw = IO.new(IO.sysopen(c, File::RDWR), "r+")
rw.seek(0)
rw.write("HE")
rw.close
p File.read(c)

app = IO.new(IO.sysopen(c, "a"), "a")
app.write("!")
app.close
p File.read(c)

# The failure names CRuby's own syscall, `rb_sysopen`.
begin
  IO.sysopen(File.join(dir, "nope.txt"), "r")
rescue SystemCallError => e
  p [e.class, e.message.sub(dir, "DIR")]
end

# TWO IO objects over ONE live descriptor, both autoclosing. Ruby's second
# close(2) fails EBADF: explicitly, that is the exception below; at teardown it
# is silence. zeo let Rust's `File` close the fd on drop, and a failed close
# there ABORTS -- so this program printed everything and then died at exit.
d = File.join(dir, "d.txt")
File.write(d, "content")
f = File.open(d)
g = IO.new(f.fileno)
p g.read
p [f.closed?, g.closed?]
f.close
begin
  g.close
rescue SystemCallError => e
  p [e.class, e.message]
end

# The pair that must still reach teardown with nothing to say.
h = File.open(d)
i = IO.new(h.fileno)
p i.read

FileUtils.remove_entry(dir)
puts "ran to the end"
__END__
"600"
"640"
"written"
"HEllo"
"HEllo!"
[Errno::ENOENT, "No such file or directory @ rb_sysopen - DIR/nope.txt"]
"content"
[false, false]
[Errno::EBADF, "Bad file descriptor"]
"content"
ran to the end
