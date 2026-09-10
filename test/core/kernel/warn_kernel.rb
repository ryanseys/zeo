# `warn arg, ...` writes each argument to stderr with a newline.
#
# at_exit + warn -- both used to fall through to the unresolved-call
# warning. warn's stderr output is asserted via the .err.expected file
# alongside this test. at_exit now runs in LIFO after main returns (#990).
at_exit { puts "from at_exit" }
warn "hello stderr"
puts "after warn"
warn "two", "args"
puts "done"
__END__
after warn
done
from at_exit
#@ stderr
hello stderr
two
args
