puts "hello world".gsub(/[aeiou]/, "a" => "1", "e" => "2", "o" => "3")
p "hello".split("")
p "hello world"[/(\w+) (\w+)/, 2]
p "Hello".casecmp?("HELLO")
p "0x1f".oct
s = "hello"; s.slice!(1, 2); p s
p({ b: 2, a: 1 }.sort)
__END__
h2ll3 w3rld
["h", "e", "l", "l", "o"]
"world"
true
31
"hlo"
[[:a, 1], [:b, 2]]
