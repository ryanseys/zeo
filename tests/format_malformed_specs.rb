p(format("%.*f", -2, 3.14159))
r001 = (format("%") rescue $!.class); p r001
r002 = (format("%1$s %s", "a", "b") rescue $!.class); p r002
r003 = (format("%d %<a>d", 1, { a: 2 }) rescue $!.class); p r003
p(format("%*d", -6, 42))
r004 = (format("%z", 1) rescue $!.class); p r004
r005 = (format("%s") rescue $!.class); p r005
p(format("%.2f", 3.14159))
p(format("%1$s %1$s", "a"))
p(format("%<a>d", { a: 2 }))
