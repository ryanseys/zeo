# Mutation THROUGH a container read: the element is the shared handle, so
# the container (and any source local) observes it.
arr = [+"hello", +"world"]
arr[0].upcase!
arr[1] << "!"
p arr
h = { k: +"value" }
h[:k].gsub!("l", "L")
p h[:k]
s = +"shared"
box = [+"x"]
box << s
box[1].reverse!
p s
p box
box[0].succ!
p box[0]
# hash string keys stay snapshots (CRuby dups+freezes stored keys)
k = +"key"
hs = { k => 1 }
k << "-mut"
p hs.keys
p k
