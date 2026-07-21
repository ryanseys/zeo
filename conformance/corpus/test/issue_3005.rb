dir = "/tmp/sp_issue_3005_#{Process.pid}"
Dir.mkdir(dir) unless File.directory?(dir)
f = File.join(dir, "f")
File.write(f, "hello")
File.chmod(0644, f)

p File.owned?(f)
# grpowned? is platform-dependent: on Linux a new file gets the process egid,
# but on macOS a /tmp file inherits the directory's group, so assert only that
# it answers a boolean rather than its exact value.
p [true, false].include?(File.grpowned?(f))
p File.setuid?(f)
p File.setgid?(f)
p File.sticky?(f)
p File.socket?(f)
p File.blockdev?(f)
p File.chardev?(f)
p File.world_readable?(f)
p File.world_writable?(f)

link = File.join(dir, "lnk")
File.symlink("f", link)
p File.symlink?(link)
p File.readlink(link)

hard = File.join(dir, "hard")
p File.link(f, hard)
p File.read(hard)

p File.utime(1000000000, 1000000000, f)
p File.mtime(f).to_i

p File.umask.is_a?(Integer)

File.delete(link)
File.delete(hard)
File.delete(f)
Dir.rmdir(dir)
puts "ok"
