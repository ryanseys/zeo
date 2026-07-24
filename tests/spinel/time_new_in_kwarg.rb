# Time.new(..., in: <off>): the keyword is the utc_offset, like the 7th
# positional (#2718). Literal "+HH:MM" resolves at compile time; an Integer
# expression passes through; missing civil fields default.
p Time.new(2020, 1, 1, 0, 0, 0, in: "+05:00").utc_offset
p Time.new(2020, 1, 1, 0, 0, 0, in: "-08:00").utc_offset
p Time.new(2020, in: "+05:00").utc_offset
p Time.new(2020, 6, 15, 12, 0, 0, in: "+05:30").strftime("%:z")
p Time.new(2020, 1, 1, 0, 0, 0, in: 3600).utc_offset
