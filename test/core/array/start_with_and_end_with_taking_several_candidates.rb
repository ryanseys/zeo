# Each ORs its arguments, on strings read out of an each_line chain.
p "ab\ncd\n".each_line.each_with_object([]) { |raw, acc| acc << raw.start_with?("a", "c") }
p "ab\ncd\n".each_line.each_with_object([]) { |raw, acc| acc << raw.end_with?("b\n", "z") }
p "ab\ncd\n".each_line.each_with_object([]) { |raw, acc| acc << raw.start_with?("a") }
__END__
[true, true]
[true, false]
[true, false]
