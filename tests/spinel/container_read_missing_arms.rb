# Three shapes of the same gap: a value whose type is only known at run time
# reaches an emitter keyed on the static type, finds no arm, and either raises
# NoMethodError naming exactly what the receiver is, or answers the slot's
# default.

# `h[k] ||= v` USED AS A VALUE. The store test has to be spelled per slot type:
# an mrb_int's nil is SP_INT_NIL, not 0, so a plain `!x` both skipped the store
# on an absent key and would overwrite a legitimately stored 0. The statement
# form always spelled it correctly; the value form did not.
h = { 0 => 0, 1 => 7 }
p(h[5] ||= 5)
p h
p(h[0] ||= 99)     # 0 is truthy in Ruby: no store
p h
p(h[1] ||= 99)
p(h[9] ||= 0)
p h

s = { "a" => "x" }
p(s["b"] ||= "y")
p(s["a"] ||= "z")
p s

# compact / flatten on an Array read out of a container. uniq had a poly arm
# and these did not.
g = { 0 => [1, nil, 2], 1 => [[3], [4, nil]] }
p g[0].compact
p g[0].flatten
p g[1].flatten
p g[0].uniq
p g[0].size

rows = [[1, nil, 2]]
p rows[0].compact

# delete_prefix / delete_suffix on a String arriving through a poly slot. The
# poly String surface covered only the zero-argument transforms.
f = Fiber.new { Fiber.yield("t=1.5"); nil }
v = f.resume
p v.delete_prefix("t=")
p v.delete_suffix("1.5")
p v.upcase

box = { "k" => "pre-body-post" }
p box["k"].delete_prefix("pre-")
p box["k"].delete_suffix("-post")
