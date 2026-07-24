# Issue #910: sub / gsub with a hash replacement argument.
# The hash's matched-substring value is the replacement; missing
# keys substitute with the empty string (CRuby semantics).
puts "hello".sub(/l/, "l" => "X")
puts "foo bar baz".sub(/\w+/, "foo" => "XXX")
puts "hello".sub("l", "l" => "X")
# gsub form (previously worked).
puts "hello".gsub(/l/, "l" => "X")
