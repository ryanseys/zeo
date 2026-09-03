# The process-family probe: the backtick child spins until main
# creates its flag file, and main can only run if the thread parked
# in read_to_end/wait(2) released the armed Gvl -- without the
# release this is a three-way deadlock (thread waits on child, child
# waits on main, main waits on the Gvl) and the test hangs.
#@ env: ZEO_GVL=1

flag = File.join(ENV["TMPDIR"] || "/tmp", "zeo_gvl_probe_#{Process.pid}")
t = Thread.new { `until [ -e #{flag} ]; do sleep 0.05; done; echo done` }
sleep 0.2
File.write(flag, "go")
p t.value
File.delete(flag)
__END__
"done\n"
