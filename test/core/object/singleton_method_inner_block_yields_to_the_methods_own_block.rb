# Inside a singleton method, an ORDINARY block's `yield` targets the
# method's own (call-site) block -- the lexical-clone path composing over
# the method-body lambda's call-site `__blk`.

obj = Object.new
def obj.sum_pairs
  total = 0
  [1, 2, 3].each { |n| total += yield(n) }
  total
end
p obj.sum_pairs { |n| n * 10 }
__END__
60
