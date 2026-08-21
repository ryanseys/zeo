# An empty hash literal takes the variant its key selects, and that mark is
# permanent -- it only ever widens afterwards. For a String key the rule reads
# the slot's own `h[k] = v` writes so a concrete value type is not thrown away;
# for an Integer key it did not, because until `[]=` joined the rule's name list
# an Integer-keyed literal never reached it. From then on a hash built only by
# writes was boxed both ways. Correct, and one representation too wide -- which
# is why only the infer-test property gate saw it.
def build
  h = {}
  h[10] = 1
  h[500] = 2
  h
end
t = build
t[3] = 7
p t[10] + t[500] + t[3]
p t.size

def named
  n = {}
  n[1] = "one"
  n[2] = "two"
  n
end
p named[1] + named[2]

# A value type the Int-keyed variants cannot hold still takes the boxed one.
def mixed
  m = {}
  m[1] = "a"
  m[2] = 3
  m
end
p mixed[1]
p mixed[2]
