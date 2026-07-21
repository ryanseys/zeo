e = SignalException.new(9, "custom")
p e.message
p e.signo
p e.signm
p SignalException.new(9).message
r = begin; SignalException.new("KILL", "custom"); rescue => x; x.class; end
p r
