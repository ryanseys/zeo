require "date"

d = Date.new(2024, 1, 31)
p (d >> 1).to_s
p (d >> 12).to_s
p (d << 1).to_s
p (d << 13).to_s
