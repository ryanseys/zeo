# map! REPLACES every element, so a block answering a value the receiver's
# element type cannot hold has to widen the receiver -- the same reasoning a
# push of a foreign element already gets. Without it the typed setter took the
# tail raw and the C build failed.
a = [1, 2]
a.map! { |x| x.to_s }
p a

b = [1, 2]
b.map! { |x| [x] }
p b

c = ["a", "bb"]
c.map! { |s| s.length }
p c

d = [1.5, 2.5]
d.collect! { |x| x > 2 }
p d

# A tail the element type DOES hold keeps the narrow array.
e = [1, 2]
e.map! { |x| x * 2 }
p e
p e.sum

f = [1, 2, 3]
f.map! { |x| x.to_s }
p f.join("-")
