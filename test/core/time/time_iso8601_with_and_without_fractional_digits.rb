# Zero, three and six digits, and a value that has to be truncated.
# (spinel issue #3095)
t = Time.utc(2001, 2, 3, 4, 5, 6, 500000)
puts t.iso8601
puts t.iso8601(3)
puts t.iso8601(6)
t2 = Time.utc(2001, 2, 3, 4, 5, 6, 123456)
puts t2.iso8601(3)
puts t2.iso8601(9)
__END__
2001-02-03T04:05:06Z
2001-02-03T04:05:06.500Z
2001-02-03T04:05:06.500000Z
2001-02-03T04:05:06.123Z
2001-02-03T04:05:06.123456000Z
