# all? and map over each_with_index, where the receiver is a parameter
# defaulting to [].
# (spinel issue #3243)
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
__END__
true
false
[1, 3, 5]
2
