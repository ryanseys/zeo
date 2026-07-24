r = (begin; h = {a: 1}.freeze; h[:b] = 2; rescue FrozenError => e; e.receiver; end)
p r
h2 = { "k" => 1 }.freeze
r2 = begin; h2.delete("k"); rescue FrozenError => e; e.receiver.equal?(h2); end
p r2
S = Struct.new(:a)
s = S.new(1).freeze
r3 = begin; s.a = 2; rescue FrozenError => e; e.receiver.equal?(s); end
p r3
