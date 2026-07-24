puts Regexp.new("a", Regexp::EXTENDED).options
puts Regexp.new("a", Regexp::MULTILINE).options
puts Regexp.new("a", Regexp::IGNORECASE).options
puts Regexp.new("a", Regexp::IGNORECASE | Regexp::EXTENDED).options
puts Regexp.new("a", 0).options
puts Regexp.new("a").options
puts(Regexp.new("A", Regexp::IGNORECASE) =~ "xxaXX" ? "i-matches" : "no")
puts(Regexp.new("a b", Regexp::EXTENDED) =~ "xab" ? "x-matches" : "no")
puts(Regexp.new("a.b", Regexp::MULTILINE) =~ "a\nb" ? "m-matches" : "no")
