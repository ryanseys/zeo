puts "before"
pr001 = proc { return 1 }
r001 = (pr001.call rescue $!.class)
p r001
puts "after"
