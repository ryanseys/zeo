hm001 = :even
r001 = (1.5.round(half: hm001) rescue $!.class); p r001
r002 = (1.5.round(half: nil) rescue $!.class); p r002
hm003 = :even; r003 = (1.5.floor(half: hm003) rescue $!.class); p r003
hm004 = :even; r004 = (1.5.round(2, half: hm004) rescue $!.class); p r004
r005 = (1.5.round(2, half: nil) rescue $!.class); p r005
p(1.5.round(half: :even))
p(2.5.round(half: :even))
p(2.5.round(half: :up))
p(2.5.round(half: :down))
