# A Float in the microseconds slot, whole and fractional, literal and from a
# local.
puts Time.utc(2001,2,3,4,5,6,500.5).nsec
puts Time.gm(2001,2,3,4,5,6,250000.5).nsec
puts Time.utc(2001,2,3,4,5,6,500).nsec
u = 123.5
puts Time.utc(2001,2,3,4,5,6,u).nsec
__END__
500500
250000500
500000
123500
