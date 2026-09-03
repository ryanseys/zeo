puts "a,b,,c".split(/,/)
puts "---"
puts ",a,b".split(/,/)
puts "---"
puts "a1b2c3".split(/\d/)
__END__
a
b

c
---

a
b
---
a
b
c
