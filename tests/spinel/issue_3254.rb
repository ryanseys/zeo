# A multi-argument String#start_with? / #end_with? on a poly string (a String
# element read out of an each_line -> each_with_object chain) must dispatch on
# the string tag, ORing the candidates, not raise NoMethodError.
p "ab\ncd\n".each_line.each_with_object([]) { |raw, acc| acc << raw.start_with?("a", "c") }
p "ab\ncd\n".each_line.each_with_object([]) { |raw, acc| acc << raw.end_with?("b\n", "z") }
p "ab\ncd\n".each_line.each_with_object([]) { |raw, acc| acc << raw.start_with?("a") }
