# Overriding a BUILTIN iterator at runtime has to take effect.
#
# Two layers had to move, because a builtin receiver reaches neither of the
# ones the object receiver uses. Dispatch: the value MRO walk probed the
# registry reopens and the builtin table per ancestor but never the overlay,
# so the definition sat there unread -- a runtime `define_method` now outranks
# both, PER ancestor, which is ruby's placement rule (an `Enumerable` override
# must not beat `Array`'s own row). Codegen: `3.times` and `(1..3).each` on
# literal receivers fuse into native counted loops, which are not calls at
# all, so nothing is left for the Path 1 de-optimization to stand down from --
# the nomination has to be withheld while the sites are chosen.

t = 0
3.times { |i| t += i }
Integer.send(:define_method, :times) { |&b| b.call(99); self }
r = []
3.times { |i| r << i }
p [t, r]

a = [1, 2]
s = []
a.each { |v| s << v }
Array.send(:define_method, :each) { |&b| b.call(:patched); self }
s2 = []
a.each { |v| s2 << v }
p [s, s2]

h = { x: 1 }
hk = []
h.each { |k, v| hk << [k, v] }
Hash.send(:define_method, :each) { |&b| b.call(:hpatched, nil); self }
hk2 = []
h.each { |k, v| hk2 << [k, v] }
p [hk, hk2]

rg = []
(1..3).each { |i| rg << i }
Range.send(:define_method, :each) { |&b| b.call(:rpatched); self }
rg2 = []
(1..3).each { |i| rg2 << i }
p [rg, rg2]

# A per-object singleton on the very array being iterated is closer still.
c = [7, 8]
c1 = []
c.each { |v| c1 << v }
def c.each; yield :singleton; self; end
c2 = []
c.each { |v| c2 << v }
p [c1, c2]

# The fused loop stands down for a NON-literal receiver too, and for one
# reached through a variable.
n = 3
n2 = []
n.times { |i| n2 << i }
p n2

# A name nothing patches keeps its fused loop -- this is the common case, and
# the reason the nomination is withheld per NAME rather than switched off.
sum = 0
4.upto(6) { |i| sum += i }
p sum
__END__
[3, [99]]
[[1, 2], [:patched]]
[[[:x, 1]], [[:hpatched, nil]]]
[[1, 2, 3], [:rpatched]]
[[:patched], [:singleton]]
[99]
15
