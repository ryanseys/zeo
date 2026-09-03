# A Float epoch is stored EXACTLY (Ruby keeps the double's true rational,
# denominator a power of two), and `nsec` is a TRUNCATED VIEW of it. This is
# observable: `Time.at(10.8) - 0.9` is nsec 900000000, which a `(sec, nsec)`
# representation gets wrong (899999999) by dropping 10.8's sub-nanosecond
# tail before subtracting. All oracle-read.

p Time.at(0.5).subsec
p Time.at(10.8).subsec
p Time.at(10.8).nsec
p (Time.at(10.8) - 0.9).nsec
p (Time.at(10.8) - 0.9).subsec
p Time.at(1.25).to_f
p Time.at(1700000000).getutc.subsec
p (Time.at(100) + -1.3).usec
p (Time.at(100) - 1.3).usec
__END__
(1/2)
(225179981368525/281474976710656)
800000000
900000000
(8106479329266899/9007199254740992)
1.25
0
699999
699999
