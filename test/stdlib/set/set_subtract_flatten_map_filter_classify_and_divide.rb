require "set"
s = Set[1, 2, 3, 4]
p s.subtract([2, 3]).to_a.sort
p s.replace([9, 8, 8, 7]).to_a.sort
p Set[Set[1, 2], Set[3, Set[4]]].flatten.to_a.sort
p Set[Set[1, 2], Set[3]].flatten!.to_a.sort
p Set[1, 2].flatten!
m = Set[1, 2, 3]; m.map! { |x| x * 10 }; p m.to_a.sort
p Set[1, 2, 3, 4].select! { |x| x.even? }.to_a.sort
p Set[2, 4].select! { |x| x.even? }
p Set[1, 2, 3, 4].reject! { |x| x.even? }.to_a.sort
p Set[1, 2, 3, 4, 5].classify { |x| x % 3 }.transform_values { |v| v.to_a.sort }
p Set[1, 2, 3, 4].divide { |i| i % 3 }.map { |g| g.to_a.sort }.sort
p Set[1, 2, 3, 4].divide { |x, y| (x - y).abs == 1 }.map { |g| g.to_a.sort }.sort
__END__
[1, 4]
[7, 8, 9]
[1, 2, 3, 4]
[1, 2, 3]
nil
[10, 20, 30]
[2, 4]
nil
[1, 3]
{1 => [1, 4], 2 => [2, 5], 0 => [3]}
[[1, 4], [2], [3]]
[[1, 2, 3, 4]]
