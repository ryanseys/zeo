require "date"

d = Date.new(2024, 2, 29)
p d.julian?
p d.gregorian?
p d.start
p Date::ENGLAND
p Date::ITALY
p d.england.to_s
p d.italy.to_s
__END__
false
true
2299161.0
2361222
2299161
"2024-02-29"
"2024-02-29"
