# A String reached through an ACCESSOR call is the same object the container
# holds, exactly as one reached through `[]` is. The shared-mutable-string
# analysis keyed only off `[]`, so the accessor route handed out a copy: the
# index assignment had no `[]=` to call on it, and the in-place mutators
# silently left the container alone (#4013).
b = [+"xyz"]
b.first[1] = "*"
p b

b = [+"xyz"]
b.last[1] = "*"
p b

b = [+"xyz"]
b.fetch(0)[1] = "*"
p b

b = [+"abc"]
b.first.replace("zzz")
p b

b = [+"abc"]
b.first << "Z"
p b

h = { k: +"abc" }
h.dig(:k) << "Z"
p h

# bound to a local first, which is the same alias
b = [+"abc"]
a = b.first
a << "Z"
p b

b = [+"abc"]
a = b.last
a.replace("zzz")
p b

# the COUNT forms answer a new Array and are untouched by any of this
b = [+"abc", +"def"]
p b.first(1)
p b.last(2)

# and the ordinary reads keep their values
p [1, 2, 3].first
p [1, 2, 3].last
p [].first
p [].last
p({ a: 1 }.first)
p (1..3).first
p [1, 2, 3].send(:first)
