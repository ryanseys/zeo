# Float rounding answers a type decided by the VALUE of ndigits, not by
# whether an argument was given: Integer when ndigits is absent or <= 0, and
# Float when it is > 0. So `round(0)` and `round(-1)` are Integers.
p 1.9.round
p 1.9.round.class
p 1.9.round(0)
p 1.9.round(0).class
p 1234.5.round(-1)
p 1234.5.round(-1).class
p 1.234.round(2)
p 1.234.round(2).class
p 1.5.ceil(0)
p 1.5.ceil(0).class
p 1.23.floor(1)
p 1.23.floor(1).class
p 19.9.truncate(-1)
p 19.9.truncate(-1).class
__END__
2
Integer
2
Integer
1230
Integer
1.23
Float
2
Integer
1.2
Float
10
Integer
