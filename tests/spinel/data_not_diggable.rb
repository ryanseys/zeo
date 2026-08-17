D = Data.define(:v)
r = ([D.new(2)].dig(0, :v) rescue $!.class)
p r

r = (D.new(2).dig(:v) rescue $!.class); p r          # Ruby: NoMethodError   Spinel: 2
h = { k: D.new(2) }
r = (h.dig(:k, :v) rescue $!.class); p r             # Ruby: TypeError       Spinel: 2

S = Struct.new(:w)
p([S.new(3)].dig(0, :w))                             # => 3
r = ([1].dig(0, 0) rescue $!.class); p r             # => TypeError
r = ([1.5].dig(0, 0) rescue $!.class); p r           # => TypeError
r = ({ a: 1 }.dig(:a, :b) rescue $!.class); p r      # => TypeError

S9 = Struct.new(:v)
p([S9.new(4)].dig(0, :v))
p({ k: S9.new(5) }.dig(:k, :v))
p(S9.new(6).dig(:v))
