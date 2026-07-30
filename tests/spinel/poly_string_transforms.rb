# sub / gsub / tr / squeeze on a String that arrives boxed (a Fiber#resume
# value, a container read), alongside the arms that already worked.
f = Fiber.new { Fiber.yield("t=1.5"); nil }
v = f.resume
p (v.sub("t=", "") rescue $!.class)
p (v.gsub("=", ":") rescue $!.class)
p (v.tr("=", "-") rescue $!.class)
p (v.squeeze("1") rescue $!.class)
p v.delete_prefix("t=")
p v.upcase
p v.split("=")
