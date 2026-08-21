# filter_map on a receiver known only at run time -- a hash value, a nested
# array element, a container read. Every other collector in the family has an
# arm for a poly receiver; filter_map had none, so the call fell past all of
# them into the hash-face coercion, which reads the receiver as a Hash and
# raises NoMethodError naming the method it was about to call (#4007):
#
#   {0 => [[]]}.map { |_, g| p g.filter_map{} }
#   # undefined method 'filter_map' for an instance of Array
#
# The block body is not the trigger -- the reported spelling used an empty one
# because it came out of #4006, but a full block fails identically.
{ 0 => [[]] }.map do |_, g|
  p g.filter_map {}
end

{ 0 => [1, nil, 2] }.map do |_, g|
  p g.filter_map { |x| x }
end

{ 0 => [1, 2, 3] }.each do |_, g|
  p g.filter_map { |x| x if x.odd? }
end

# a nested array element is the same shape without a hash
[[1, nil, 2]].map { |g| p g.filter_map { |x| x } }

# and so is a plain container read
h = { 0 => [1, nil, 2] }
g = h[0]
p g.filter_map { |x| x }
p g.filter_map {}

# a forwarded proc reaches the runtime enumerable helper rather than a spliced
# loop, and that surface was missing filter_map too
f = ->(x) { x if x.odd? }
{ 0 => [1, 2, 3] }.map { |_, k| p k.filter_map(&f) }

# the receiver really can be a Hash, and then filter_map is over its pairs
{ 0 => { a: 1, b: nil } }.map do |_, g|
  p g.filter_map { |k, v| v && k }
end
p({ a: 1, b: nil }.filter_map { |k, v| v && k })

# the typed forms still answer
p [1, nil, 2].filter_map { |x| x }
p [1, 2, 3].filter_map { |x| x if x.odd? }
