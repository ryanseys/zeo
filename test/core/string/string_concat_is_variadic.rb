s = "a"
s.concat("b", "c", "d")
puts s
t = "x"
t << "y" << "z"
puts t
u = "n"
u.concat(65, 66)
puts u
puts "keep".concat
__END__
abcd
xyz
nAB
keep
