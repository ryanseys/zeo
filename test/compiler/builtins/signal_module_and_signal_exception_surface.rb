# SignalException/Interrupt resolve a signal name<->number (#signo/#signm,
# arg-form validation) and the Signal module answers list/signame/trap
# (trap is a validated no-op that records the prior action). Byte-verified
# against ruby 4.0.6 on darwin.

p Interrupt.new.signo
p Interrupt.new.message
p Interrupt.new("stop").signm
e = SignalException.new(9, "custom"); p [e.signo, e.message, e.signm]
p SignalException.new(9).message
p SignalException.new("INT").signo
p SignalException.new(:TERM).message
p((SignalException.new("KILL", "x") rescue $!.class))
p((SignalException.new("NOPE") rescue $!.class))
begin; raise SignalException, "SIGINT"; rescue SignalException => x; p x.signo; end
p Signal.list["INT"]
p Signal.list.class
p Signal.signame(15)
p Signal.signame(2.9)
p Signal.signame(999)
p((Signal.signame(nil) rescue $!.class))
p Signal.trap("USR1", "IGNORE")
p Signal.trap("USR1", "DEFAULT")
p((Signal.trap("KILL", "IGNORE") rescue $!.class))
p Signal.class
__END__
2
"Interrupt"
"stop"
[9, "custom", "custom"]
"SIGKILL"
2
"SIGTERM"
ArgumentError
ArgumentError
2
2
Hash
"TERM"
"INT"
nil
TypeError
"DEFAULT"
"IGNORE"
Errno::EINVAL
Module
