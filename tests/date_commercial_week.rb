require "date"

d = Date.new(2024, 2, 29)
p d.cwday
p d.cweek
p d.cwyear
p Date.new(2021, 1, 1).cwyear
