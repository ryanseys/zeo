s1 = "hello"; r1 = (s1.encode!("UTF-8") rescue $!.class); p r1
s2 = "hello"; r2 = (s2.force_encoding("UTF-8") rescue $!.class); p r2
p "hello".encode("UTF-8")
p "hello".b
m = +"hello"; p m.force_encoding("UTF-8"); p m.encode!("UTF-8")
