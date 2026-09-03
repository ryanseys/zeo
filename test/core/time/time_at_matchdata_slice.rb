# Time.at with a unit-scaled fractional second (:millisecond/:microsecond/
# :nanosecond); the default unit is microseconds.
p Time.at(0, 500, :millisecond).to_f
p Time.at(0, 500, :microsecond).to_f
p Time.at(0, 500, :nanosecond).to_f
p Time.at(1, 250).to_f

# MatchData#[] slices the group array with (start, length) or a range.
md = "2024-01-31".match(/(\d+)-(\d+)-(\d+)/)
p md[1, 2]
p md[0, 2]
p md[1..]
p md == "2024-01-31".match(/(\d+)-(\d+)-(\d+)/)

# Float scientific notation zero-pads the exponent to two digits.
p 5.0e-7
p 1.0e20
__END__
0.5
0.0005
5.0e-07
1.00025
["2024", "01"]
["2024-01-31", "2024"]
["2024", "01", "31"]
true
5.0e-07
1.0e+20
