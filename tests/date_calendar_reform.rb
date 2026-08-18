require "date"

d = Date.new(2024, 2, 29)
p d.julian?
p d.gregorian?
p d.start
p Date::ENGLAND
p Date::ITALY
p d.england.to_s
p d.italy.to_s
