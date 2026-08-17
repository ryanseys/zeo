p("a1b2".gsub(/(?<l>[a-z])(?<n>\d)/) { $~[:n] })
z001 = []; "a1b2".scan(/(?<l>[a-z])(?<n>\d)/) { z001 << $~[:n] }; p z001
p("a1b2".gsub(/([a-z])(\d)/) { $~[2] })
p("a1b2".gsub(/(?<l>[a-z])(?<n>\d)/) { "#{$2}#{$1}" })
p("a1b2".gsub(/(?<l>[a-z])(?<n>\d)/) { $~.captures.join("-") })
"hello" =~ /(?<x>l+)/; p $~[:x]
z = []; "a1b2".scan(/([a-z])(\d)/) { z << $~[2] }; p z
z2 = []; "a1b2".scan(/([a-z])(\d)/) { z2 << $1 }; p z2
z3 = []; "a1b2".scan(/(?<l>[a-z])(?<n>\d)/) { z3 << $~.captures }; p z3
