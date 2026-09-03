def show(src, flags)
  Regexp.new(src, flags)
  puts "compiled"
rescue RegexpError => e
  puts e.message
end

show('[z-a]', 0)
show('[z-a]', Regexp::IGNORECASE)
show('[z-a]', Regexp::EXTENDED)
show('[z-a]', Regexp::MULTILINE)
show('[z-a]', Regexp::IGNORECASE | Regexp::EXTENDED)
show('[z-a]', Regexp::IGNORECASE | Regexp::MULTILINE)
show('[z-a]', Regexp::EXTENDED | Regexp::MULTILINE)
show('[z-a]', Regexp::IGNORECASE | Regexp::EXTENDED | Regexp::MULTILINE)

show('(?i)[z-a]', 0)
show('(?-i)[z-a]', Regexp::IGNORECASE)
show('(?m)[z-a]', 0)
show('(?-m)[z-a]', Regexp::MULTILINE)

p Regexp.new('a', Regexp::IGNORECASE | Regexp::EXTENDED | Regexp::MULTILINE).inspect
p Regexp.new('a', Regexp::MULTILINE).inspect
p Regexp.new('a', 0).inspect

show('(?<1>a)', Regexp::MULTILINE)
show('[a-\d]', Regexp::IGNORECASE)
show('\R', Regexp::EXTENDED)
__END__
empty range in char class: /[z-a]/
empty range in char class: /[z-a]/i
empty range in char class: /[z-a]/x
empty range in char class: /[z-a]/m
empty range in char class: /[z-a]/ix
empty range in char class: /[z-a]/mi
empty range in char class: /[z-a]/mx
empty range in char class: /[z-a]/mix
empty range in char class: /(?i)[z-a]/
empty range in char class: /(?-i)[z-a]/i
empty range in char class: /(?m)[z-a]/
empty range in char class: /(?-m)[z-a]/m
"/a/mix"
"/a/m"
"/a/"
invalid group name <1>: /(?<1>a)/m
char-class value at end of range: /[a-\d]/i
compiled
