def f001(n001, memo001 = Hash.new)
  memo001[n001] = n001
  memo001
end
p f001(1)
def f002(n, memo = Hash.new(0))
  memo[n] += 1
  memo
end
p f002(:a)
h003 = Hash.new
h003[:a] = 1
p h003
