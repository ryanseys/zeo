# find_index / index / rindex WITH A BLOCK on a poly receiver.
#
# The block form is emitted by the typed-array path, which is keyed on the
# storage kind, so a value only known to be an array at run time never reached
# it and the call fell through to the unresolved-call raise. Every sibling name
# -- find, select, count, any? -- already had a poly loop of its own; this
# family had none, on either side: analyze did not type the call either, so
# even once it emitted, the value was discarded.

def pick(f)
  f ? [3, 4, 5, 6] : "no"
end

a = pick(true)
p a.find_index { |x| x > 4 }
p a.index { |x| x > 4 }
p a.rindex { |x| x > 4 }
p a.find_index { |x| x > 99 }
p a.rindex { |x| x > 99 }
p a.find_index { |x| true }

# strings, so the element rides the pointer channel
def pick_s(f)
  f ? ["aa", "b", "ccc"] : 0
end

s = pick_s(true)
p s.find_index { |x| x.length == 1 }
p s.rindex { |x| x.length >= 2 }

# a truthy non-boolean block value: every type but nil/false is truthy
p a.find_index { |x| x }
p a.find_index { |x| nil }

# the sibling names still answer as before
p a.find { |x| x > 4 }
p a.count { |x| x > 4 }

# a hash receiver: entries arrive as pairs
def pick_h(f)
  f ? { "a" => 1, "b" => 2 } : 0
end

h = pick_h(true)
p h.find_index { |k, v| v == 2 }

# the argument form is untouched
p a.index(5)
