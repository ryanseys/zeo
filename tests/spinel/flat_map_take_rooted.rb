# flat_map's receiver is the array the loop walks, and the block body allocates
# (the result array, each spliced value). A freshly built receiver -- here the
# array `take` returns -- is held by nothing, so a collection inside the loop
# freed it and the walk stopped early. Run under SPINEL_GC_STRESS=1 to see the
# short count without the root.
rings = [
  [-64.5630852, 46.2938887],
  [-64.5630733, 46.2939199],
]
struct = Struct.new :x, :y

rings.map do |lon, lat|
  struct.new lon, lat
end.group_by do |v|
  [v.y, v.x]
end.then do |grouped|
  puts "bins: #{grouped.size}"
  n = 0
  grouped.each_with_index.take(5000).flat_map do |((i, j), g), _i|
    n += 1
    g.map do |vertex|
      []
    end
  end
  p n
end

a = [[1, 2], [3, 4], [5, 6]]
m = 0
a.each_with_index.take(5000).flat_map do |(x, y), i|
  m += 1
  []
end
p m
