p({1 => [2]}.map { |k, g| g.map {} })

box = [[2, 3]]
p(box.map { |g| g.map {} })
p(box.map { |g| g.map { |x| } })

nested = [["a"], ["b", "c"]]
p(nested.map { |g| g.map {} })
p(nested.map { |g| g.map { |s| s.upcase } })
