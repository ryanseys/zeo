# A Hash.new called on directly has no key usage to decide its variant, and
# staying unknown made every method on it an unresolved call (#3823).
begin
  p Hash.new(0).fetch(:z)
rescue KeyError => e
  p e.class
end
p Hash.new(0)[:z]
p Hash.new(0).size
p Hash.new(0).empty?
p Hash.new { |h, k| k.to_s }[:abc]
h = Hash.new(0)
h[:a] += 1
p h[:a]
p h[:b]
# values_at answers the default for a missing key
p Hash.new(0).values_at(:x, :y)
p({ a: 1 }.values_at(:a, :b))
