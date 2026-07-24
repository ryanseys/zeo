h = {a: 1, b: 2}
p h.filter_map { |k, v| [k, v] if v > 1 }
p h.filter_map { |k, v| v * 10 if v > 1 }
p [1, 2, 3].filter_map { |x| [x] if x.odd? }
p({x: 5}.filter_map { |k, v| nil })
