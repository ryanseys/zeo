class Graph
  def key(x) = x % 3
  def buckets = (0...6).group_by { |x| key(x) }
end
p Graph.new.buckets
p (0...6).group_by { |x| x % 3 }
p %w[a bb ccc dd].group_by { |s| s.length }
