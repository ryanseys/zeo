p (1..5).size
p (1...5).size
p (1..).size
p (1..5.5).size
p (1...5.5).size
p (10..1).size
p Proc.new { |x| x * 3 }.call(4)
__END__
5
4
Infinity
5
5
0
12
