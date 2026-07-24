a = [1, 2].freeze
r = begin; a << 3; rescue FrozenError => e; e.receiver.equal?(a); end
p r
r2 = begin; [1].freeze << 2; rescue FrozenError => e; e.receiver; end
p r2
s = ["x"].freeze
r3 = begin; s.push("y"); rescue FrozenError => e; e.receiver; end
p r3
h = { a: 1 }.freeze
r4 = begin; h[:b] = 2; rescue FrozenError => e; e.class; end
p r4
