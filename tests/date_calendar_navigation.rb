require "date"

d = Date.new(2024, 1, 31)
p d.next_month.to_s
p d.prev_month.to_s
p d.next_year.to_s
p d.prev_year.to_s
p d.next_month(2).to_s
