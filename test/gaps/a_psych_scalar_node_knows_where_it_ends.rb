# A parsed Psych node reports where it starts AND where it ends, in lines
# and columns. zeo's parser records no marks at all, so every node answers
# 0:0 for both; the first start matches only by chance.
require "psych"

doc = Psych.parse("a: hello\nb: 2\n")
key = doc.root.children[0]
puts "scalar start #{key.start_line}:#{key.start_column}"
puts "scalar end   #{key.end_line}:#{key.end_column}"
puts "mapping start #{doc.root.start_line}:#{doc.root.start_column}"
puts "mapping end   #{doc.root.end_line}:#{doc.root.end_column}"
__END__
scalar start 0:0
scalar end   0:1
mapping start 0:0
mapping end   2:0
