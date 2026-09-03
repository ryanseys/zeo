p String.new
p String.new("hi")
p String.new.encoding.name
p String.new("x", encoding: "ASCII-8BIT").encoding.name
__END__
""
"hi"
"ASCII-8BIT"
"ASCII-8BIT"
