m = "hello world".match(/(\w+) (\w+)/)
puts m.to_a
puts "---"
puts m.captures
__END__
hello world
hello
world
---
hello
world
