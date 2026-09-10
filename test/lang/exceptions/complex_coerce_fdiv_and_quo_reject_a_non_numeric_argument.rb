# Complex#coerce, #fdiv and #quo each raise for an argument that is not a number.
p((Complex(2, 3).coerce("x") rescue $!.class))
p((Complex(2, 3).fdiv("x") rescue $!.class))
p((Complex(2, 3).quo(nil) rescue $!.class))
__END__
TypeError
TypeError
TypeError
