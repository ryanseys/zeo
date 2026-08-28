# Ruby zero-pads a year to four digits in Time#to_s, #inspect and
# strftime("%Y"); C's strftime does not, so a year below 1000 came out
# "1-01-01 00:00:00 UTC" where CRuby writes "0001-01-01 00:00:00 UTC".
# Surfaced by #4144's probe, which made these slots reachable.
[[1, 1, 1], [9, 2, 3], [99, 3, 4], [999, 12, 31], [1000, 1, 1],
 [2024, 1, 1], [10000, 1, 1]].each do |y, m, d|
  t = Time.utc(y, m, d)
  puts [t.to_s, t.inspect, t.strftime("%Y"), t.year, t.strftime("%Y-%m-%d")].join(" | ")
end

# A local time takes the same path with an offset rather than "UTC".
t = Time.utc(1, 6, 15, 12, 30, 45)
p t.to_s
p t.strftime("%Y/%m/%d %H:%M:%S")

# The year is still an Integer, unpadded, wherever it is asked for as one.
p Time.utc(99, 1, 1).year
p Time.utc(99, 1, 1).year.to_s

# Padding is not width-forcing: a five-digit year keeps all five.
p Time.utc(10000, 1, 1).strftime("%Y")

# Fractional seconds still render after the seconds, not the year.
p Time.utc(1, 1, 1, 0, 0, 0).inspect
