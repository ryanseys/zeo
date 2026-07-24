# String#lines / #each_line take an optional separator and a chomp: keyword.
p "one\ntwo\nthree".lines
p "one\ntwo\nthree".lines(chomp: true)
p "a\r\nb\r\n".lines(chomp: true)     # default sep also strips a preceding \r
p "a|b|c".lines("|")

seen = []
"red\ngreen\nblue\n".each_line(chomp: true) { |line| seen << line.upcase }
p seen

# String#prepend accepts any number of strings, inserted in order at the front.
greeting = "world"
greeting.prepend("hello, ", "dear ")
p greeting
