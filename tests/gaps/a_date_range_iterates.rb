# A Range of Dates iterates via #succ (`to_a`, `each`, `step`); zeo
# raises TypeError "can't iterate from Date". (Found by the 2026-08-24
# probe sweep.)
require "date"
r = Date.new(2001, 1, 1)..Date.new(2001, 1, 3)
p r.to_a.length
p r.map(&:day)
