# The `acc = []` default is an array the caller can go on using.
# (spinel issue #2919)
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
__END__
1
[3]
[1, 2, 4]
10
