# Enumerable chain/reverse_each/compact/to_set on user classes + Array#chain (#2474-2478)
class C
  include Enumerable
  def each; yield 3; yield 1; yield 2; end
end
c = C.new
a = []; v = a
p(v.chain([1, 2]).to_a)      # 2474 Array#chain, empty array in a variable
c.reverse_each { |x| print x }; puts  # 2475
p(c.compact)                 # 2476
p(c.chain([9]).to_a)         # 2477
require "set"
p(c.to_set)                  # 2478
