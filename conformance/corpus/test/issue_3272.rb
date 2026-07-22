require "ostruct"
o = OpenStruct.new(a: 1)
p o.frozen?
o.b = 2
p o.b
o.freeze
p o.frozen?
begin
  o.c = 3
  puts "no raise (member)"
rescue FrozenError
  puts "member frozen"
end
begin
  o[:d] = 4
  puts "no raise (index)"
rescue FrozenError
  puts "index frozen"
end
p o.a
p o.b
