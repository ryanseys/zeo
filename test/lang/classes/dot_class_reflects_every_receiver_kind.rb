class Widget
end
module Helper
end

w = Widget.new
puts w.class
puts w.class == Widget
puts w.class.name
puts 5.class
puts "s".class
puts [].class
puts nil.class
puts true.class
puts 1.5.class
puts :sym.class
puts (1..2).class
puts({}.class)
puts Helper.class
puts Widget.class
puts Class.class
puts Widget.class == Class
__END__
Widget
true
Widget
Integer
String
Array
NilClass
TrueClass
Float
Symbol
Range
Hash
Module
Class
Class
true
