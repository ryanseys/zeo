# each_with_index terminals over a union-typed (defaulted `= []`) receiver.
def safe?(placement = [])
  placement.each_with_index.all? { |c, r| c > r }
end
p safe?([1, 2, 3])
p safe?([5, 0, 9])

def idx_map(a = [])
  a.each_with_index.map { |c, r| c + r }
end
p idx_map([1, 2, 3])

def big_count(a = [])
  a.each_with_index.count { |c, r| c > r }
end
p big_count([5, 0, 9])
