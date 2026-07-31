# Overriding a BUILTIN iterator at runtime has no effect: `Integer#times`,
# `Array#each`, `Hash#each` and `Range#each` keep answering with the compiled
# body after a `define_method` replaces them, and so does a method reached
# through a module already in the receiver's ancestry.
#
# Not introduced by narrowing the deopt latch -- output is byte-identical
# before and after. The runtime definition lands in the overlay, but a builtin
# receiver resolves through the frozen value-method tables, which no lookup
# consults the overlay alongside. A per-object singleton on the very receiver
# DOES win (the last block), because that lookup is identity-keyed.
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

# A module in the receiver's ancestry, not the receiver's own class.
b1 = []
[4, 5].each { |v| b1 << v }
Enumerable.send(:define_method, :map) { |&blk| [:enum_patched] }
p [b1, [4, 5].map { |v| v * 2 }]

# A per-object singleton on the very array being iterated.
c = [7, 8]
c1 = []
c.each { |v| c1 << v }
def c.each; yield :singleton; self; end
c2 = []
c.each { |v| c2 << v }
p [c1, c2]
