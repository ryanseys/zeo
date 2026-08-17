require "date"

d = Date.new(2024, 2, 27)
p d.upto(Date.new(2024, 3, 1)).map(&:to_s)
p Date.new(2024, 3, 1).downto(d).map(&:to_s)
p d.step(Date.new(2024, 3, 4), 2).map(&:to_s)
