# `encode!` with and without an argument, and `scrub!`, each answer the
# receiver.
# (spinel issue #3086)
p("hello".encode!("UTF-8"))
p("abc".scrub!("?"))
s = "world"; s.encode!; p s
__END__
"hello"
"abc"
"world"
