[].tap do |rings|
  rings = [].take 1000
end

[].tap do |rings|
  rings = rings.take 1000
end

[].tap { |r| p r }
[].then { |r| p r }
p([].tap { |r| r })
p([].tap { |r| r.push(1) })
p([1].tap { |r| p r })
p([].then { |r| r.size })
p({}.tap { |h| p h })
