m001 = "a1".match(/(?<l>[a-z])(\d)/)
p m001.captures
p m001.to_a
p m001.size
"a1" =~ /(?<l>[a-z])(\d)/
p $~.size
p m001[:l]
p m001[1]
p(/(?<l>[a-z])(\d)/.names)
p "a1".match(/([a-z])(\d)/).captures
