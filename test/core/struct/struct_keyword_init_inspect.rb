# From the spinel corpus (c55d9bdb).
# A keyword_init Struct's inspect omits the "(keyword_init: true)" suffix.
#
K = Struct.new(:a, keyword_init: true)
J = Struct.new(:a)
DD = Data.define(:a)

p K.name
p K.to_s
p K.inspect
p K
p J.name, J.to_s, J.inspect, J
p DD.name, DD.to_s, DD.inspect, DD

k = K.new(a: 1)
p k
p k.class
p k.class.inspect
p k.class.to_s
p k.class.name
p K.keyword_init?
p J.keyword_init?

j = J.new(2)
p j.class
p "#{K}"
__END__
"K"
"K"
"K(keyword_init: true)"
K(keyword_init: true)
"J"
"J"
"J"
J
"DD"
"DD"
"DD"
DD
#<struct K a=1>
K(keyword_init: true)
"K(keyword_init: true)"
"K"
"K"
true
nil
J
"K"
