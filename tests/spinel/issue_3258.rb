module Outer; class Thing; end; end
r1 = (5.is_a?(::Integer) rescue "raise:#{$!.class}")
r2 = ("s".is_a?(Outer::Thing) rescue "raise:#{$!.class}")
r3 = ("s".is_a?(::String) rescue "raise:#{$!.class}")
r4 = (5.is_a?(Outer::Thing) rescue "raise:#{$!.class}")
o = Outer::Thing.new
r5 = (o.is_a?(Outer::Thing) rescue "raise:#{$!.class}")
r6 = (o.is_a?(Integer) rescue "raise:#{$!.class}")
p r1; p r2; p r3; p r4; p r5; p r6
