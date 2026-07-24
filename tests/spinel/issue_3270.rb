require "ostruct"
o = OpenStruct.new
o.x = 1
puts o.inspect
o.y = "hi"
puts o.inspect
o2 = OpenStruct.new(a: 1, b: 2)
puts o2.inspect
# stored result keeps its snapshot (no dangling / later-mutation bleed)
o3 = OpenStruct.new(a: 1)
s = o3.inspect
o3.b = 2
puts s
puts o3.to_s
