# Open3.capture2 raises NoMethodError ("undefined method 'spawn' for module
# Open3") instead of running the subprocess and capturing its output -- the
# vendored Open3 stub is missing the underlying spawn/popen support that
# capture2 depends on (the same gap as IO.popen, from a different entry
# point).
require "open3"
out, status = Open3.capture2("echo", "hi")
p out
p status.exitstatus
