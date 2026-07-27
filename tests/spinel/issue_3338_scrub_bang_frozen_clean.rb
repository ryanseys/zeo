s1 = "hello"; r1 = (s1.scrub! rescue $!.class); p r1
s2 = "hello"; r2 = (s2.scrub!("?") rescue $!.class); p r2
p("hello".scrub!)
s3 = "abc\x80def"; r3 = (s3.scrub!("?") rescue $!.class); p r3
m = +"abc\x80def"; p m.scrub!("?")
