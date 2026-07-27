# The last three corner cases (P6): bytesplice/append_as_bytes share,
# poly-container iteration mutates string elements via runtime dispatch,
# and guard-narrowed poly receivers keep the shared handle.
s = +"hello"
t = s
s.bytesplice(0, 1, "J")
p t
u = +"ab"
v = u
u.append_as_bytes("c", 100)
p v
box = [+"xy"]
box[0].bytesplice(0, 1, "Z")
p box
arr = [+"s1", +"s2", 3]
arr.each { |x| x << "!" if x.is_a?(String) }
p arr
