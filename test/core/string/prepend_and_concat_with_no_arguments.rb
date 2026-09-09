# Each answers the receiver on a mutable string, and raises on a frozen one.
# (spinel issue #3339)
m1 = +"hello"; r1 = (m1.prepend rescue $!.class); p r1
f1 = "hello";  r2 = (f1.concat  rescue $!.class); p r2
f2 = "hello";  r3 = (f2.prepend rescue $!.class); p r3
__END__
"hello"
"hello"
"hello"
