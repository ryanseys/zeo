# IO.popen -- a subprocess with a pipe replacing its stdout ("r"), its stdin
# ("w"), or both ("r+"), the handle knowing its child: #pid answers it and
# #close reaps it into $?.
IO.popen("echo hello") { |io| puts io.read.strip }
p $?.exitstatus

io = IO.popen(["printf", "a b c"])
p io.read
p io.pid.positive?
p io.closed?
io.close
p $?.success?

# A shell string with a metacharacter runs through the shell.
IO.popen("echo one && echo two") { |io| p io.read.split }

# Write mode: the child's stdin is ours; the close delivers its EOF.
IO.popen("wc -c > /dev/null", "w") { |io| io.write("12345") }
p $?.exitstatus

# Duplex: one handle, both directions. close_write hands the child EOF while
# its answer stays readable.
IO.popen("cat", "r+") do |io|
  io.write("round trip\n")
  io.close_write
  p io.read
end

# A leading env hash reaches the child.
IO.popen({ "POPEN_PROBE" => "seen" }, "echo $POPEN_PROBE") { |io| puts io.read }

# The block's value is the call's answer.
p(IO.popen("true") { |io| io.pid.is_a?(Integer) })
__END__
hello
0
"a b c"
true
false
true
["one", "two"]
0
"round trip\n"
seen
true
