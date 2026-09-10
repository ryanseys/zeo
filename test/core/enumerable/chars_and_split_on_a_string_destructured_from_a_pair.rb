# map over pairs calling chars, split, chars.length and to_s on the first
# element.
r = [["ab", "cd"]].map { |a, b| a.chars }
p r
p [["a,b", "c"]].map { |a, b| a.split(",") }
p [["ab", "cd"]].map { |a, b| a.chars.length }
p [[:x, :y]].map { |a, b| a.to_s }
__END__
[["a", "b"]]
[["a", "b"]]
[2]
["x"]
