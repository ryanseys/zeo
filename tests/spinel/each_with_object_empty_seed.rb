p({ a: 1 }.each_with_object([]) { |x, a| a })

p({ a: 1 }.each_with_object({}) { |x, a| a })          # Ruby: {}   Spinel: compile abort
p({}.each_with_object([]) { |x, a| a })                # Ruby: []   Spinel: compile abort
v = { a: 1 }.each_with_object([]) { |x, a| a }; p v    # Ruby: []   Spinel: compile abort

p({ a: 1 }.each_with_object([1]) { |x, a| a })         # => [1]
p({ a: 1 }.each_with_object(0) { |x, a| a })           # => 0
p({ a: 1 }.each_with_object("s") { |x, a| a })         # => "s"
p({ a: 1 }.each_with_object([]) { |x, a| a << x })     # => [[:a, 1]]
p({ a: 1 }.each_with_object([]) { |x, a| a.push(1) })  # => [1]
p({ a: 1 }.inject([]) { |a, x| a })                    # => []

p({ a: 1, b: 2 }.each_with_object({}) { |x, a| a[x[0]] = x[1] })
p({ a: 1 }.each_with_object([]) { |(k, v), a| a << k })
p([1, 2].each_with_object([]) { |x, a| a })
p((1..2).each_with_object([]) { |x, a| a })
