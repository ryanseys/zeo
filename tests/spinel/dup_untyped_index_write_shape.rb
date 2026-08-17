# A `dup` of a statically unknown receiver keeps the SOURCE's shape: an int-keyed
# `[]=` into the copy is no evidence that it is a hash, and guessing one made the
# method answer `{}` where CRuby answered an Array (#3952).
def build(node, n)
  copy = node.dup
  copy[0] = n.zero? ? 7 : build(node[0], n - 1)
  copy
end
p build([], 0)
p build([[1]], 1)

def store(src)
  copy = src.dup
  copy[0] = "v"
  copy
end
p store([])
p store(["a", "b"])
p store({})
p store({ 0 => "z", 1 => "y" })
