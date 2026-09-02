require "date"

d = Date.new(2024, 2, 29)
puts d, d + 1, d >> 12, d.leap?, d.jd, d.strftime("%Y-%j %A")
puts Date.parse("2001-02-03"), Date.strptime("03/04/2005", "%d/%m/%Y")
dt = DateTime.new(2024, 1, 1, 12, 30, 15)
puts dt.iso8601, dt.to_time.utc.to_i
m = Marshal.load(Marshal.dump(d))
puts m == d, m.class, (Date.new(2024, 3, 1) - d).to_i, Date.valid_date?(2023, 2, 29)
puts Date.new(2024, 12, 25).cwday, Date.new(2024, 12, 25).yday, d.next_month.to_s
