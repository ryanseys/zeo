p({}.each_entry { |e| e })

p({ a: 1, b: 2 }.each_entry { |e| e })    # Ruby: {a: 1, b: 2}   Spinel: no output
p({ "a" => 1 }.each_entry { |e| e })      # Ruby: {"a" => 1}     Spinel: no output

h = { a: 1, b: 2 }
o = []; h.each_entry { |e| o << e }; p o   # => [[:a, 1], [:b, 2]]
c = (h.each_entry { |e| e }); p c          # Ruby: {a: 1, b: 2}   Spinel: {a: 1, b: 2}
h2 = {}; o2 = []; h2.each_entry { |e| o2 << e }; p o2   # => []
p([1, 2].each_entry { |e| e })             # => [1, 2]
