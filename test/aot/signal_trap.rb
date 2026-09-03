# A trap handler runs in a linked binary: the signal handler tables and the
# self-pipe survive the link.
got = false
Signal.trap("USR1") { got = true }
Process.kill("USR1", Process.pid)
sleep 0.05 until got
puts "handled USR1"
Signal.trap("USR1", "DEFAULT")
puts Signal.list.key?("USR1")
__END__
handled USR1
true
