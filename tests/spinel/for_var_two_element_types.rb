# A `for` loop variable is an ordinary local, so two loops binding the same
# name share one C slot. It was typed from whichever loop the pass reached
# last, and the other then assigned a String element into an sp_int slot. The
# slot now holds the union of every loop's element type, and a typed array's
# element is boxed into it (#4168).
def cmd(models)
  out = []
  for n in models.keys.sort
    out.push(n)
  end
  for n in [1, 2]
    out.push(n)
  end
  for n in (0..1)
    out.push(n)
  end
  for n in [1.5]
    out.push(n)
  end
  out
end
p cmd({ "b" => 1, "a" => 2 })

# single loop of each kind still keeps its concrete slot
def one
  t = 0
  for i in [1, 2, 3]
    t += i
  end
  s = ""
  for w in ["x", "y"]
    s += w
  end
  [t, s]
end
p one

# a for over a range and over a hash
h = { "k" => 1 }
for pair in h
  p pair
end
for a, b in [[1, 2], [3, 4]]
  p [a, b]
end
