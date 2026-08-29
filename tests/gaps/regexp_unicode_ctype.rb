p "\xB5".b =~ /\b/
p "\xB5".b =~ /[[:word:]]/
p "\xC2\xB5".b =~ /[[:word:]]/
p "\xC2\xB5".b.scan(/\b/).size
