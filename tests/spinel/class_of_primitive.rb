# Issue #715. `<primitive>.class` should return the builtin class for
# each primitive, not just for obj-typed receivers. Coverage:
# Integer/Float/String/Symbol/NilClass/TrueClass/FalseClass/Array/Hash.

puts 42.class
puts 3.14.class
puts "hi".class
puts :ok.class
puts nil.class
puts true.class
puts false.class
puts [1, 2].class
h = {"a" => 1}
puts h.class
