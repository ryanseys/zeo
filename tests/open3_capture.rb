# Open3 over the real spawn machinery: Kernel#spawn taking IO redirect
# targets (the pipe ends popen2/popen3 build), plus Process.detach's wait
# thread carrying the child's Process::Status.
require "open3"

out, status = Open3.capture2("echo", "hi")
p out
p status.exitstatus

out, err, status = Open3.capture3("sh", "-c", "echo to-out; echo to-err >&2; exit 3")
p out
p err
p status.exitstatus

Open3.popen2("cat") do |stdin, stdout, wait_thr|
  stdin.puts "through the pipes"
  stdin.close
  p stdout.read
  p wait_thr.value.success?
end
