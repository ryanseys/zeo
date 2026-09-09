# nil.clone(freeze: true) and a bare clone both answer nil; freeze: false raises.
# (spinel issue #3033)
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
__END__
ArgumentError
nil
nil
"hi"
false
true
