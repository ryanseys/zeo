# String#split without limit drops trailing empty strings
# String#split(sep, -1) keeps them (CRuby default vs no-limit behavior).
# String#split(sep, 0) is "no limit" and drops trailing empties too;
# only a negative limit keeps them.

puts "a,b,c,".split(",").inspect
puts "a,b,c,,,".split(",").inspect
puts "a,b,c".split(",").inspect
puts "a,b,c,".split(",", -1).inspect
puts "a,b,c,".split(",", 0).inspect
puts "a,b,c,,".split(",", 0).inspect
puts "a,b,c,".split(",", 2).inspect
