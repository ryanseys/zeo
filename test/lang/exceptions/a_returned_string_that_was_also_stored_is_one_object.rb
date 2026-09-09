# A method that stores a mutable string AND returns it: the caller's binding
# is the same object, so a later append is visible through both. Also with
# two candidates and a branch choosing which to return.
# (spinel issue #3227)
$store = []
def make_held
  s = +"kept"
  $store << s
  s << "-in"
  s
end
r = make_held
r << "-out"
p r
p $store[0]
p r.equal?($store[0])

# multiple returns, all shared
def pick(flag)
  a = +"aa"
  b = +"bb"
  $store << a
  $store << b
  a << "1"
  b << "2"
  return a if flag
  b
end
x = pick(true)
x << "!"
p $store[1]
y = pick(false)
y << "?"
p $store[4] rescue p $store.last
__END__
"kept-in-out"
"kept-in-out"
true
"aa1!"
"bb2?"
