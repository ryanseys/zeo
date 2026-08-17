f001 = ->(m, k) { m << k }
p(["a"].each_with_object([]) { |k001, m001| f001.call(m001, k001) })
f002 = ->(m, k) { m[k] = 1 }
p(["a"].each_with_object({}) { |k002, m002| f002.call(m002, k002) })
f003 = ->(m, k) { p m }
["a"].each_with_object({}) { |k003, m003| f003.call(m003, k003) }
f005 = ->(m, k) { m << k }
p([1].inject([]) { |m005, k005| f005.call(m005, k005) })
p([1, 2].each_with_object([]) { |x, m| m << x * 2 })
p([1, 2].inject(0) { |a, x| a + x })
