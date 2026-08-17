p([].map { |x| 1 })

p([].each_with_object([]) { |x, a| a })     # Ruby: []    Spinel: undeclared identifier 'lv_x'
p([].each_with_object(0) { |x, a| a })      # Ruby: 0     Spinel: undeclared identifier 'lv_x'
p([].each_with_object("s") { |x, a| a })    # Ruby: "s"   Spinel: undeclared identifier 'lv_x'
p([].each_with_object([]) { |_x, a| a })    # Ruby: []    Spinel: undeclared identifier 'lv__x'

p([].map { |x| x })                             # => []
p([1].map { |x| 1 })                            # => [1]
a = []; p(a.map { |x| 1 })                      # => []
p([].each { |x| 1 })                            # => []
p([].each_with_object([]) { |x, a| a << x })    # => []
p([1].each_with_object([]) { |x, a| a })        # => []
p([].each_with_index { |x, i| i })              # => []

p([].select { |x| 1 })
p([].reject { |x| true })
p([].flat_map { |x| [1] })
p([].each_with_index { |x, i| 1 })
p([].sort_by { |x| 1 })
p({}.map { |k, v| 1 })
p([].map { |x| x })
p([1, 2].map { |x| 1 })
