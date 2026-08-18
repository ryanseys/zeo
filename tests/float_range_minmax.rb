r001 = ((1.0..5.0).minmax rescue $!.class); p r001
a002 = (1.0..5.0); r002 = (a002.minmax rescue $!.class); p r002
p((1.0..5.0).min)
p((1.0..5.0).max)
r003 = ((1.0...5.0).minmax rescue $!.class); p r003
p((1..5).minmax)
p(("a".."e").minmax)
