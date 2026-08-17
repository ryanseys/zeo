class T; def base; 1; end; end
o = T.new
def o.extra; 99; end
d = o.dup
p d.respond_to?(:extra)
p o.respond_to?(:extra)
c = o.clone
p c.respond_to?(:extra)
p d.base
p o.extra
p c.extra
p d.class
