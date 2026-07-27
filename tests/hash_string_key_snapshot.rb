# A String KEY is snapshotted on store: the hash keeps a frozen copy, so
# mutating the string you stored under can never rewrite a key already in the
# table. A stored VALUE is the opposite -- the shared handle, mutable through
# the hash. Both halves matter, so both are asserted here.

# --- the snapshot, through every way a key gets in ---------------------------
k = "key"

literal = { k => 1 }
assign = {}
assign[k] = 1
stored = {}
stored.store(k, 1)
merged = {}.merge(k => 1)
built = [[k, 1]].to_h
bracketed = Hash[k, 1]
folded = [k].each_with_object({}) { |s, h| h[s] = 1 }

k << "-mut"

p literal.keys, assign.keys, stored.keys, merged.keys
p built.keys, bracketed.keys, folded.keys
p k

# The copy is frozen; the caller's string is untouched.
h = {}
s = "fresh"
h[s] = 1
p h.keys.first.frozen?
p s.frozen?
p h.keys.first.equal?(s)

# An already-frozen key needs no copy -- it is stored as-is.
f = "immutable".freeze
h2 = { f => 1 }
p h2.keys.first.equal?(f)

# Lookup still works by content after the caller mutates its own string.
h3 = {}
name = "ada"
h3[name] = 1
name << "!"
p h3["ada"]
p h3["ada!"]
p h3.size

# --- the copy carries the key's own encoding, not a UTF-8 reinterpretation ---
bin = "caf\xC3\xA9".b
h4 = { bin => :binary }
p h4.keys.first.encoding
p h4.keys.first.bytes
p h4.keys.first == bin

# --- identity keying keeps YOUR object: its identity IS the key --------------
h5 = {}.compare_by_identity
id_key = "same"
h5[id_key] = 1
h5["same"] = 2
p h5.size
p h5.keys.first.equal?(id_key)
p h5.keys.first.frozen?

# --- VALUES stay shared: mutation through the hash is visible ----------------
v = { k: "value" }
v[:k] << "!"
p v[:k]
shared = "outer"
v[:s] = shared
v[:s].upcase!
p shared
p v[:s].frozen?

# --- non-String keys are stored as-is ----------------------------------------
arr = [1, 2]
h6 = { arr => :a }
arr << 3
p h6.keys
p h6.keys.first.equal?(arr)

# --- dup/clone of a hash keep the frozen keys, no re-copy --------------------
h7 = { "d" => 1 }
p h7.dup.keys.first.equal?(h7.keys.first)
p h7.clone.keys.first.frozen?
