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
