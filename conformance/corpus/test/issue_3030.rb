p((KeyError.new("m").key rescue $!.class))
p((KeyError.new("m").receiver rescue $!.class))
p((FrozenError.new("m").receiver rescue $!.class))
p((NoMethodError.new("m").receiver rescue $!.class))
p((KeyError.new("m", key: :k, receiver: {}).key rescue $!.class))
p((Interrupt.new.signo rescue $!.class))
h = {a: 1}
p((h.fetch(:z) rescue $!.key))
p((h.fetch(:z) rescue $!.receiver))
