# Time.at(Time) copies the instant; the 10-arg to_a order; the string
# form (offset / UTC / local / fractional); and UTC vs a numeric +00:00
# (which render differently and disagree on #utc?).

p Time.at(Time.at(55)).to_i
p Time.at(Time.at(1.5)).nsec
p Time.utc(1, 15, 20, 1, 1, 2000, 0, 0, 0, 0).inspect
t = Time.new("2021-12-25 10:00:00 +09:00")
p t.utc_offset
p t.inspect
u = Time.new("2021-12-25 10:00:00 UTC")
p u.utc?
p u.inspect
f = Time.new("2021-12-25 10:00:00.5 +00:00")
p f.nsec
p f.utc?
p f.inspect
__END__
55
500000000
"2000-01-01 20:15:01 UTC"
32400
"2021-12-25 10:00:00 +0900"
true
"2021-12-25 10:00:00 UTC"
500000000
false
"2021-12-25 10:00:00.5 +0000"
