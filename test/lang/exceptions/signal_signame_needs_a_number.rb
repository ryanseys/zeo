# Signal.signame raises for nil, true, a String and a Symbol, and answers a name for an Integer.
p((Signal.signame(nil) rescue $!.class))
p((Signal.signame(true) rescue $!.class))
p((Signal.signame("INT") rescue $!.class))
p((Signal.signame(:INT) rescue $!.class))
p(Signal.signame(9))
__END__
TypeError
TypeError
TypeError
TypeError
"KILL"
