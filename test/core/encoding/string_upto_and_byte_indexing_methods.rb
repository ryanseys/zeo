p "a8".upto("b1").to_a
r = []
"a".upto("e") { |x| r << x }
p r
p "hello".byteindex("l")
p "hello".byteindex("l", 3)
p "hello".byterindex("l")
p "hello".byterindex("l", 2)
p "hello".byteslice(1, 3)
p "café".byteslice(0, 3)
p "hello".byteslice(-2, 2)
p "hello".byteslice(10)
__END__
["a8", "a9", "b0", "b1"]
["a", "b", "c", "d", "e"]
2
3
3
2
"ell"
"caf"
"lo"
nil
