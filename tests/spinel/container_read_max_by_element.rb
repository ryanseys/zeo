# max_by / min_by on an Array reached through a container return the winning
# element, typed like the one sort_by(...).first or find return -- not an
# untyped value every later call rejects.
S = Struct.new(:n) do
  def m = n * 2
end
g = { "k" => [S.new(1), S.new(3)] }
p g["k"].max_by { |x| x.m }.n
p g["k"].min_by { |x| x.m }.n
p g["k"].max_by(&:m).n
p g["k"].min_by(&:m).n
buckets = [S.new(1), S.new(2), S.new(3), S.new(4)].group_by { |x| x.n % 2 }
p buckets[0].max_by { |x| x.n }.n
p buckets[1].min_by { |x| x.n }.n
h = { "a" => [3, 1, 2] }
p h["a"].max_by { |v| v }
p h["a"].min_by { |v| v }
p h["a"].max_by { |v| -v }
e = { "e" => [] }
p e["e"].max_by { |v| v }.nil?
