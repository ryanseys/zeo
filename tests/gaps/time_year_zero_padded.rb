[[1, 1, 1], [9, 2, 3], [99, 3, 4], [999, 12, 31], [1000, 1, 1],
 [2024, 1, 1], [10000, 1, 1]].each do |y, m, d|
  t = Time.utc(y, m, d)
  puts [t.to_s, t.inspect, t.strftime("%Y"), t.year, t.strftime("%Y-%m-%d")].join(" | ")
end

t = Time.utc(1, 6, 15, 12, 30, 45)
p t.to_s
p t.strftime("%Y/%m/%d %H:%M:%S")

p Time.utc(99, 1, 1).year
p Time.utc(99, 1, 1).year.to_s

p Time.utc(10000, 1, 1).strftime("%Y")

p Time.utc(1, 1, 1, 0, 0, 0).inspect
