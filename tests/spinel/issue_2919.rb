# A method returning a default-valued array parameter: the empty `[]` default
# types the param (and the method's return) as a poly array, so the result
# responds to Array methods.
def make(n, acc = [])
  acc << n
  acc
end
p make(3).length
p make(3)
p make(4, [1, 2])
def build(x, acc = [])
  acc.push(x * 2)
  acc
end
p build(5).first
