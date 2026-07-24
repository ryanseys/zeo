p Float::INFINITY
r = (Float.const_get(:INFINITY) rescue $!.class); p r
p Math::PI
r2 = (Math.const_get(:PI) rescue $!.class); p r2
