# `join`'s separator is a NILABLE String slot: nil is legal and means "" (it
# is `$,`'s default), false and every other class are a TypeError. The
# emitters passed the argument into the const char* slot raw, so both spellings
# reached the join as a NULL and it read that as a string -- a segfault either
# way, for every array kind (found reviewing PR #4029, whose sweep did not
# reach this slot).
def try
  yield
rescue => e
  p [e.class, e.message]
end

p [1, 2].join(nil)
p ["a", "b"].join(nil)
p [1.5, 2.5].join(nil)
p [[1], [2]].join(nil)
p [1, [2, 3]].join(nil)
p([:a, :b].join(nil))
sep = nil
p [1, 2].join(sep)
p [1, 2].join
p [1, 2].join("-")
p ["a", "b"].join("")

try { [1, 2].join(false) }
try { [1, 2].join(true) }
try { [1, 2].join(1) }
try { ["a"].join(:x) }

# through a poly receiver, where the array kind is a run-time question
def pick(f) = f ? [1, 2] : "s"
p pick(true).join(nil)
try { pick(true).join(false) }
