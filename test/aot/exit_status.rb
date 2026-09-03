# The linked binary's exit path: `exit` unwinds, at_exit still runs, and the
# status reaches the shell.
at_exit { $stderr.puts "leaving" }
puts "before"
exit 3
puts "unreachable"
__END__
before
#@ stderr
leaving
#@ exit 3
