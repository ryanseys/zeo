puts "one two three".scan(/\w+/)
puts "---"
puts "a1b2c3".scan(/([a-z])(\d)/)
__END__
one
two
three
---
a
1
b
2
c
3
