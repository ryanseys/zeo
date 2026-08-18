r001 = ((1..5).first(-1) rescue $!.class); p r001
r002 = ((1..5).last(-1) rescue $!.class); p r002
r003 = ((1..5).min(-1) rescue $!.class); p r003
r004 = ((1..5).max(-1) rescue $!.class); p r004
a005 = (1..5); n005 = -1; r005 = (a005.first(n005) rescue $!.class); p r005
r010 = (("a".."e").first(-1) rescue $!.class); p r010
p (1..5).first(2)
p (1..5).last(2)
p (1..5).min(2)
p (1..5).max(2)
