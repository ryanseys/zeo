# A method returning Hash.new(default) where the caller pins the int-keyed,
# float-valued shape (PolyPolyHash): the backprop exclusion this variant
# needed before PolyPolyHash_new_with_default existed is gone.
def mk; Hash.new(0.0); end
x = mk
x[1] += 2.5
p x[1]
p x[9]
p x.default
