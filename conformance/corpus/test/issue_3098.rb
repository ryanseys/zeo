a001 = (throw :x rescue $!); b001 = (throw :y rescue $!); p(a001 == b001)
c001 = (raise "x" rescue $!); d001 = (raise "y" rescue $!); p(c001 == d001)
e001 = (throw :x rescue $!); p(a001 == e001.class)
