# Array#to_s on a destructured element, on an indexed one, and through `map(&:to_s)`.
# (spinel issue #3007)
[[:x, :y]].each { |e| p e.to_s }
x = [[:x, :y]]
p x[0].to_s
p [[:a], [:b, :c]].map(&:to_s)
p [[1, 2], [:s]].map(&:to_s)
p [[:x, :y]].to_s
__END__
"[:x, :y]"
"[:x, :y]"
["[:a]", "[:b, :c]"]
["[1, 2]", "[:s]"]
"[[:x, :y]]"
