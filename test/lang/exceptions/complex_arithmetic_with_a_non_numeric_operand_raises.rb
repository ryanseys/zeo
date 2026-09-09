# Complex + - * / against a String, nil or true each raise TypeError.
# (spinel issue #2963)
p((Complex(2, 3) + "x" rescue $!.class))
p((Complex(2, 3) - nil rescue $!.class))
p((Complex(2, 3) * true rescue $!.class))
p((Complex(2, 3) / "x" rescue $!.class))
__END__
TypeError
TypeError
TypeError
TypeError
