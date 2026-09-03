p "hello".byteslice(1..3)
p "hello".byteslice(2..)
p "hello".byteslice(10..12)
a = [1, 2, 3, 4]
p a.slice!(1..2)
p a
b = [0, 0, 0, 0, 0]
b.fill(1..2) { |i| i + 100 }
p b
__END__
"ell"
"llo"
nil
[2, 3]
[1, 4]
[0, 101, 102, 0, 0]
