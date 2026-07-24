# Materializing an each_with_index enumerator with to_h builds {element => index}.
def si(x); x; end
def ss(x); x; end
def sp(x); x; end
p si([10, 20, 30]).each_with_index.to_h
p ss(["a", "b", "c"]).each_with_index.to_h
p si([10, 20, 30]).each.with_index(1).to_h
# A mixed poly-array receiver keeps a poly key, so the elements box into the hash.
p sp([:x, "y", 3]).each_with_index.to_h
