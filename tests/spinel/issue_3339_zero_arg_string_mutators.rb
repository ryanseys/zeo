m1 = +"hello"; r1 = (m1.prepend rescue $!.class); p r1
f1 = "hello";  r2 = (f1.concat  rescue $!.class); p r2
f2 = "hello";  r3 = (f2.prepend rescue $!.class); p r3
