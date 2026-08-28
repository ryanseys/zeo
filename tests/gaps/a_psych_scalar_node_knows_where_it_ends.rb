require "psych"

doc = Psych.parse("a: hello\nb: 2\n")
key = doc.root.children[0]
puts "scalar start #{key.start_line}:#{key.start_column}"
puts "scalar end   #{key.end_line}:#{key.end_column}"
puts "mapping start #{doc.root.start_line}:#{doc.root.start_column}"
puts "mapping end   #{doc.root.end_line}:#{doc.root.end_column}"
