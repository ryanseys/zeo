# Enumerator::Lazy#size: source size propagated through the chain (#2485)
p((1..10).lazy.size)                              # 10 (the filed repro)
p([1, 2, 3].lazy.size)                            # 3
p([].lazy.size)                                   # 0
p([1, 2, 3].lazy.map { |x| x * 2 }.size)          # 3 (map preserves)
p([1, 2, 3].lazy.select { |x| x > 1 }.size)       # nil (filter unknown)
p([1, 2, 3].lazy.take(2).size)                    # 2
p([1, 2, 3].lazy.take(5).size)                    # 3 (min)
p([1, 2, 3, 4, 5].lazy.drop(1).take(2).size)      # 2
p((1..Float::INFINITY).lazy.size)                 # Infinity
p((1..Float::INFINITY).lazy.map { |x| x }.size)   # Infinity
p((1..Float::INFINITY).lazy.take(3).size)         # 3 (take bounds)
# the fused pipeline forcing terminals still work
p((1..Float::INFINITY).lazy.map { |x| x * 2 }.first(3))
