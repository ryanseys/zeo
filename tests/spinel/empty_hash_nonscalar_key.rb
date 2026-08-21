# A bare `{}` takes the hash variant its KEY selects. The rule knew Symbol,
# String, Integer and poly keys; every other kind fell through to the
# StrPolyHash default, which put the key straight into a `const char *` slot
# and the C compiler reported it against generated code (#4000):
#
#   error: initialization of 'const char *' from incompatible pointer type
#          'sp_IntArray *'
#
# The reported spelling had an EMPTY array as the key, which is the harder
# half: it infers no kind at all, yet it is still a pointer once emitted.
p(
  [].each do
    {}.fetch [], 0
  end
)

class T
  def arr_key
    {}.fetch([1, 2], 0)
  end

  def empty_arr_key
    {}.fetch([], 0)
  end

  def empty_hash_key
    {}.fetch({}, 0)
  end

  def float_key
    {}.fetch(1.5, 0)
  end

  def nil_key
    {}.fetch(nil, 0)
  end

  def obj_key
    {}.fetch(Object.new, 0)
  end
end

t = T.new
p t.arr_key
p t.empty_arr_key
p t.empty_hash_key
p t.float_key
p t.nil_key
p t.obj_key

# and the widened variant really holds those keys, not just compiles
h = {}
h[[1, 2]] = "pair"
h[1.5] = "float"
h[nil] = "nil"
p h[[1, 2]]
p h[1.5]
p h[nil]
p h.fetch([9], "miss")
p h.key?([1, 2])
p h.dig([1, 2])
p h.size
