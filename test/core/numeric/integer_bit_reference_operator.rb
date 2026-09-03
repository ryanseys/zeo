p 0b1011[0]
p 0b1011[2]
p 5[0, 2]
p 0b1101[1, 3]
p 255[0..3]
p 255[4..]
p (-2)[0]
p (-2)[1]
p 10[100]
p((1 << 100)[100])
begin
  255[..3]
rescue ArgumentError => e
  puts e.message
end
__END__
1
0
1
6
15
15
0
1
0
1
The beginless range for Integer#[] results in infinity
