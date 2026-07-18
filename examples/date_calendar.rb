require "date"

# Construction and field access on the proleptic-Gregorian calendar.
d = Date.new(2024, 3, 15)
p [d.year, d.month, d.day]
p d.wday          # 0=Sunday .. 6=Saturday
p d.yday          # day of the year
p d.leap?
p d.to_s

# strftime with common calendar directives.
p d.strftime("%A, %B %-d, %Y")
p d.strftime("%a %b %d")

# Day arithmetic: + / - integers, and date - date -> Rational days.
p (d + 30).to_s
p (d - 15).to_s
p (Date.new(2024, 12, 31) - Date.new(2024, 1, 1))
p d.next.to_s

# Ordering (Comparable) and equality.
p (Date.new(2024, 1, 1) < Date.new(2024, 6, 1))
p (d == Date.new(2024, 3, 15))
p [Date.new(2024, 3, 1), Date.new(2024, 1, 1), Date.new(2024, 2, 1)].sort.map(&:to_s)

# Julian day and parsing.
p Date.jd(2460385).to_s
p Date.parse("2025-07-04").to_s
p Date.valid_date?(2024, 2, 29)
