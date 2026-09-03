# File.open's open(2) ITSELF blocks on a FIFO until the other end
# shows up -- the reason the release wraps the open, not just reads.
# Without it the reader parks inside the armed Gvl and main can never
# open the write end: a deadlock, and this test hangs.
#@ env: ZEO_GVL=1

fifo = File.join(ENV["TMPDIR"] || "/tmp", "zeo_fifo_probe_#{Process.pid}")
system("mkfifo", fifo)
reader = Thread.new { File.open(fifo, "r") { |f| f.gets } }
sleep 0.2
File.open(fifo, "w") { |f| f.puts "through the fifo" }
p reader.value
File.delete(fifo)
__END__
"through the fifo\n"
