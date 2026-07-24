p((Complex(2, 3).coerce("x") rescue $!.class))
p((Complex(2, 3).fdiv("x") rescue $!.class))
p((Complex(2, 3).quo(nil) rescue $!.class))
