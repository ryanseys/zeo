# SignalException built from a signal number reports #message, #signo and #signm; a name with a message raises.
# (spinel issue #3073)
e = SignalException.new(9, "custom")
p e.message
p e.signo
p e.signm
p SignalException.new(9).message
r = begin; SignalException.new("KILL", "custom"); rescue => x; x.class; end
p r
__END__
"custom"
9
"custom"
"SIGKILL"
ArgumentError
