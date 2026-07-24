r = (nil.clone(freeze: false) rescue $!.class); p r
r2 = (nil.clone(freeze: false) rescue :err); p r2
r3 = (Integer("x") rescue $!.class); p r3
r4 = (nil.foo rescue $!.class); p r4
