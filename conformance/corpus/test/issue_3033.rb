x = nil
r = begin; x.clone(freeze: false); rescue => e; e.class; end
p r
p x.clone(freeze: true)
p x.clone
def pick(b); b ? "hi" : nil; end
z = pick(true)
p z.clone(freeze: false)
o = Object.new
p o.clone(freeze: false).frozen?
p o.clone(freeze: true).frozen?
