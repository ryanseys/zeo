# It is not frozen at first, accepts a new member, and refuses one after
# freeze.
# (spinel issue #3272)
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
__END__
false
2
true
member frozen
index frozen
1
2
