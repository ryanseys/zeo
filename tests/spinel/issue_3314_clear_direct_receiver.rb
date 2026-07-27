r001 = ((+"abc").clear rescue $!.class); p r001
a001 = +"abc"; v001 = a001.clear; p v001
p((+"x").clear.empty?)
p(("frozen".clear rescue $!.class))
