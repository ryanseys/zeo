require "dup_data"

a = DupData.new(7)
b = a.dup
c = a.clone

p [a.value, a.has_struct?]
p [b.value, b.has_struct?]
p [c.value, c.has_struct?]
p [a.class, b.class, c.class]
