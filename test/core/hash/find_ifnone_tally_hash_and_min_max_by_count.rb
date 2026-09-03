p [1, 2, 3].find(-> { -1 }) { |x| x > 10 }
p [1, 20, 3].find(-> { -1 }) { |x| x > 10 }
p [1, nil, 3].find(-> { :fallback }) { |x| x.nil? }
h = Hash.new(0)
p [1, 1, 2, 3, 3, 3].tally(h)
p h
p [5, 5, 6].tally({ 5 => 10 })
p %w[bbbb a ccc dd].max_by(2, &:length)
p %w[bbbb a ccc dd].min_by(2, &:length)
p [1, 2, 3].min_by(0) { |n| n }
begin
  [1, 2, 3].max_by(-1) { |n| n }
rescue ArgumentError => e
  p e.message
end
__END__
-1
20
nil
{1 => 2, 2 => 1, 3 => 3}
{1 => 2, 2 => 1, 3 => 3}
{5 => 12, 6 => 1}
["bbbb", "ccc"]
["a", "dd"]
[]
"negative size (-1)"
