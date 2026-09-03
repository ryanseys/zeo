r = ((1..5).cover?("x") rescue $!.class)
p r

r = ((1..5).include?("x") rescue $!.class); p r
r = ((1..5).member?("x") rescue $!.class); p r
r = ((1..5) === "x" rescue $!.class); p r
r = ((1..5).first("x") rescue $!.class); p r
r = ((1..5).each_slice("x") { |s| } rescue $!.class); p r

r = ((1..5).eql?(1.0..5.0) rescue $!.class); p r

r = ((1..5) == (1.0..5.0) rescue $!.class); p r
r = ((1..5).cover?(3) rescue $!.class); p r
r = (("a".."e").include?("c") rescue $!.class); p r
r = ((1..5).take(2) rescue $!.class); p r
r = ((1..5).eql?(1..5) rescue $!.class); p r
__END__
false
false
false
false
TypeError
TypeError
false
true
true
true
[1, 2]
true
