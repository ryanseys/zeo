s = "abc".freeze
p (s.upcase! rescue $!.class); p s
p ("xyz".freeze.reverse! rescue $!.class)
p ("abc".freeze.slice!(0) rescue $!.class)
p ("abc".freeze.slice!(0..1) rescue $!.class)
p ("abc".freeze.insert(0, "x") rescue $!.class)
p ("abc".freeze.replace("z") rescue $!.class)
u = "abc"; p u.upcase!; p u
p "hello".slice!(0)
