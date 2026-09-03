# `warn`, `$stderr.puts`, and `STDERR.write` all go to the error stream;
# `$stdout.puts` stays on stdout. The two streams are asserted separately,
# so a leak in either direction fails the test.

$stdout.puts "out1"
warn "w1"
$stderr.puts "err1"
STDERR.write "err2\n"
$stdout.puts "out2"
warn "w2", "w3"
__END__
out1
out2
#@ stderr
w1
err1
err2
w2
w3
