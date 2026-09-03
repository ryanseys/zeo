big = 2 ** 100
p big.zero?
p big.positive?
p big.negative?
p big.integer?
p big.class
p big.succ
p big.pred
p((10 ** 30).to_i)
p((2 ** 100).to_i.class)
n = 0 - (2 ** 80)
p n.negative?
p n.zero?
p n.positive?
__END__
false
true
false
true
Integer
1267650600228229401496703205377
1267650600228229401496703205375
1000000000000000000000000000000
Integer
true
false
false
