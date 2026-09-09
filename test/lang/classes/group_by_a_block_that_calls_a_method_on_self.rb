# Range#group_by whose block calls another method of the same object, beside
# the plain forms over a Range and an array of strings.
# (spinel issue #2902)
class Graph
  def key(x) = x % 3
  def buckets = (0...6).group_by { |x| key(x) }
end
p Graph.new.buckets
p (0...6).group_by { |x| x % 3 }
p %w[a bb ccc dd].group_by { |s| s.length }
__END__
{0 => [0, 3], 1 => [1, 4], 2 => [2, 5]}
{0 => [0, 3], 1 => [1, 4], 2 => [2, 5]}
{1 => ["a"], 2 => ["bb", "dd"], 3 => ["ccc"]}
