p((..5).max)
r001 = ((..5).first rescue $!.class); p r001
r002 = ((1..).last rescue $!.class); p r002
r003 = ((1..).minmax rescue $!.class); p r003
r004 = ((..5).to_a rescue $!.class); p r004
p((1..).count)
p((1..).size)
p((1..5).count)
p((1..5).max)
p((1..5).first)
