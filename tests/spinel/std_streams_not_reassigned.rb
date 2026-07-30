# A program that never reassigns a standard stream must emit exactly as before:
# no global, no runtime test on the read path.
#
# The harness compares the two streams separately, so the split is the check:
# every write below lands where it was addressed.
$stdout.puts "a"
p($stdout == $stderr)
p($stdout == $stdout)
p($stderr == $stderr)
$stdout.write("w\n")
$stdout.flush
$stderr.puts "b"
$stderr.write("e\n")
$stderr.flush
