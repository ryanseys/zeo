# utc, gm and local each carry it into nsec, and inspect renders it.
# (spinel issue #3091)
p Time.utc(2020,1,1,0,0,0, Rational(1,2)).nsec
p Time.utc(2020,1,1,0,0,0, Rational(1,4)).nsec
p Time.gm(2020,1,1,0,0,0, Rational(3,2)).nsec
p Time.local(2020,1,1,0,0,0, Rational(1,2)).nsec
p Time.utc(2020,1,1,0,0,0, Rational(1,2))
__END__
500
250
1500
500
2020-01-01 00:00:00.0000005 UTC
