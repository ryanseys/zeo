r001 = (Process.times rescue $!.class); p r001.class
t = (Process.times rescue nil)
p t.class
p Process.times.class
