p("hello".encode!("UTF-8"))
p("abc".scrub!("?"))
s = "world"; s.encode!; p s
__END__
"hello"
"abc"
"world"
