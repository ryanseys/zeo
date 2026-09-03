p "a\nb\nc".lines
p "a\nb\nc".lines(chomp: true)
p "a\r\nb\r\nc\r\n".lines(chomp: true)
p "a-b-c".lines("-")
collected = []
"x\ny\n".each_line(chomp: true) { |l| collected << l }
p collected
t = "world"
t.prepend("hello ", "big ")
p t
__END__
["a\n", "b\n", "c"]
["a", "b", "c"]
["a", "b", "c"]
["a-", "b-", "c"]
["x", "y"]
"hello big world"
