def store(memo, n)
  memo[n] ||= n * 10
end
memo = {}
store(memo, 1)
store(memo, 2)
store(memo, 1)
p memo.size
p memo
# string-key ||= through a param
def store_s(h, k) = h[k] ||= "v"
hs = {}
store_s(hs, "x"); store_s(hs, "x")
p hs
