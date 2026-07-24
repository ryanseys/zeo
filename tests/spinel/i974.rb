m = /(\w+)(\s+)(\w+)/.match("hello world")
puts m.offset(0).inspect
puts m.offset(1).inspect
puts m.offset(3).inspect
puts m.begin(1)
puts m.end(1)
puts m.begin(3)
puts m.end(3)
puts m[0]
puts m[1]
puts m[3]
puts m.pre_match.inspect
puts m.post_match.inspect
puts m.captures.inspect
puts m.to_a.inspect

m2 = "xx-yy".match(/(\w+)-(\w+)/)
puts m2.begin(2)
puts m2.end(2)
puts m2.pre_match.inspect
puts m2.post_match.inspect

puts "abc".match(/z/).nil?
