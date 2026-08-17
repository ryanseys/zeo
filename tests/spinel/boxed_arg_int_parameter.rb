# A value with no static type (an sp_RbVal) reaching a builtin parameter
# declared mrb_int was emitted with no unbox step, so the generated C did not
# compile. An empty array literal as a parameter default is the shortest way to
# produce one: the parameter has no element type, so a read of it answers the
# polymorphic accessor's boxed value.
def bytes_of(bytes: [])
  s = String.new("\xFF\xFF\xFF\xFF")
  4.times { |i| s.setbyte(i, bytes[i]) }
  s.bytes
end
p bytes_of(bytes: [1, 2, 3, 4])

def string_ops(a: [])
  s = "hello world"
  [s.byteslice(a[0], a[1]),
   s.byteslice(a[0]),
   s.rindex("l", a[2]),
   s.split("o", a[1]),
   "ff".to_i(a[3])]
end
p string_ops(a: [1, 3, 8, 16])

def array_ops(a: [])
  arr = [1, 2, 3, 4]
  combos = arr.combination(a[1]).to_a.length
  perms = arr.permutation(a[1]).to_a.length
  rot = [1, 2, 3].rotate!(a[0])
  dropped = [9, 8, 7].delete_at(a[0])
  [combos, perms, rot, dropped, arr.dig(a[0])]
end
p array_ops(a: [1, 2])

def num_ops(a: [])
  [1234.round(a[0]), Integer.sqrt(a[1])]
end
p num_ops(a: [-2, 17])

# A split with a limit inside a block form takes the same route.
def split_limit(a: [])
  out = []
  "a,b,c,d".split(",", a[0]).each { |part| out << part }
  out
end
p split_limit(a: [2])
