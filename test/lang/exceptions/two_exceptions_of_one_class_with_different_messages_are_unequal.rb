# Exception#== compares the message too, so two throws with different tags are not equal.
# (spinel issue #3098)
a001 = (throw :x rescue $!); b001 = (throw :y rescue $!); p(a001 == b001)
c001 = (raise "x" rescue $!); d001 = (raise "y" rescue $!); p(c001 == d001)
e001 = (throw :x rescue $!); p(a001 == e001.class)
__END__
true
false
false
