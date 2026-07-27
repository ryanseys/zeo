# IO.select / IO#wait_* / IO.for_fd / Dir.for_fd / Dir class-constant NoMethodError
p IO.select(nil, nil, nil, 0)
r, w = IO.pipe
w.write("x")
w.flush
p IO.select([r], nil, nil, 0).class
p IO.select([r], nil, nil, 0)[0].size
p IO.select(nil, nil, [r], 0)      # no priority data: nil, not the pipe
p r.wait_readable(0).class
p w.wait_writable(0).class
p r.wait_priority(0).nil?          # a pipe with plain data is not priority-readable
p r.wait(0, :read).class
r.close
w.close

dir = "spinel_iofd_test_dir"
Dir.mkdir(dir) unless Dir.exist?(dir)
path = File.join(dir, "sample.txt")
File.write(path, "hello\n")

f = File.open(path)
i = IO.for_fd(f.fileno, autoclose: false)
p i.class
p i.fileno == f.fileno
f.close
e = (IO.for_fd(-1) rescue $!.class); p e

p Dir.exist?(dir)
n = (Dir.bogus_xyz rescue $!.class); p n
m = (Dir.bogus_xyz rescue $!.message); p m
b = (Dir.for_fd(9999) rescue $!.class); p b
c = (Dir.fchdir(9999) rescue $!.class); p c

d = Dir.for_fd(File.open(dir).fileno)
p d.class
d.close

File.delete(path)
Dir.rmdir(dir)
