# Object.new instances track frozen state (freeze/frozen?/clone-preserves);
# clone(freeze: false) on an always-frozen immediate raises ArgumentError.

o = Object.new
p o.frozen?
o.freeze
p o.frozen?
p o.clone.frozen?
p o.dup.frozen?
u = Object.new
p u.clone(freeze: true).frozen?
p u.clone(freeze: false).frozen?
p((nil.clone(freeze: false) rescue $!.class))
p((1.clone(freeze: false) rescue $!.class))
__END__
false
true
true
false
true
false
ArgumentError
ArgumentError
