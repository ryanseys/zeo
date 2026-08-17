require 'set'
p(Set[3, 1, 2].max { |x, y| y <=> x })
p(Set[3, 1, 2].min { |x, y| y <=> x })
p(Set[3, 1, 2].sort { |x, y| y <=> x })
p([3,1,2].max { |x, y| y <=> x })
p(Set[3,1,2].max(2))
p(Set[3,1,2].sort_by { |x| -x })
