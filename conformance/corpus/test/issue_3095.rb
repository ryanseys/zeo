t = Time.utc(2001, 2, 3, 4, 5, 6, 500000)
puts t.iso8601
puts t.iso8601(3)
puts t.iso8601(6)
t2 = Time.utc(2001, 2, 3, 4, 5, 6, 123456)
puts t2.iso8601(3)
puts t2.iso8601(9)
