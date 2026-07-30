# `$stderr = $stdout` has to be honoured by reads of $stderr.
#
# A program that reassigns one of the standard streams already gets a real
# global for it (gv_stderr, NULL until the assignment runs), and the assignment
# was emitted correctly. Only the READ ignored it, so the write still went to
# the real stderr and `$stderr == $stdout` answered false -- the two halves of
# the report.
#
# A program that never reassigns has no such global and emits exactly as
# before, which is what the second half of this test pins.

$stderr = $stdout
p($stderr == $stdout)
$stderr.puts "err"
puts "out"
$stderr.write("w\n")
$stderr.print("p\n")
$stderr.flush

# every one of those lines is on stdout now, so a stderr-only reader sees
# nothing; the test harness merges the two, so the order above is the check
