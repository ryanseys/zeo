# A `*` over a group that can only match empty -- a bare lookahead or
# lookbehind. ruby runs one final empty iteration and then takes the
# optional tail, so both patterns match. zeo's engine loops differently and
# the tail is never reached.
p(/(?:(?!a))*b?/.match("b").to_a)
p(/a(?:(?<=a))*b?/.match("ab").to_a)
__END__
["b"]
["ab"]
