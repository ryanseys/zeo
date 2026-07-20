# The Signal module and the SignalException/Interrupt hierarchy: name<->number
# resolution (signame/list), trap as a validated no-op returning the prior
# handler, and #signo/#signm on the exception objects.
puts Signal.list["INT"]
puts Signal.list["TERM"]
puts Signal.signame(9)
p Signal.signame(999)
p Signal.signame(2.9)
puts Signal.trap("USR1", "IGNORE")
puts Signal.trap("USR1", "DEFAULT")
p(begin; Signal.trap("NOPE", "IGNORE"); rescue => e; e.class; end)

e = SignalException.new("INT")
puts e.signo
puts e.message
puts SignalException.new(9).message
p(begin; SignalException.new("NOPE"); rescue => x; x.class; end)

i = Interrupt.new
puts i.signo
puts i.message
puts Interrupt.new("stop").message
