s1 = "abc"; r1 = (s1.append_as_bytes("de") rescue $!.class); p r1
s2 = "hello"; r2 = (s2.bytesplice(0, 2, "HE") rescue $!.class); p r2
s3 = "hello"; r3 = (s3.bytesplice(0..1, "HE") rescue $!.class); p r3
s4 = "abc\x80def"; r4 = (s4.scrub!("?") rescue $!.class); p r4
p "abc\x80def".scrub("?")
m1 = +"abc"; m1.append_as_bytes("de"); p m1
m2 = +"hello"; m2.bytesplice(0, 2, "HE"); p m2
