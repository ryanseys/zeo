# The binary spawns a child, waits for it and reads its status: fork, exec
# and waitpid all run from the linked runtime.
pid = spawn("/bin/echo", "from the child")
_, status = Process.wait2(pid)
puts status.exitstatus
puts status.success?
out = IO.popen(["/bin/echo", "captured"], &:read)
print out
failed = spawn("/bin/sh", "-c", "exit 7")
puts Process.wait2(failed).last.exitstatus
__END__
from the child
0
true
captured
7
