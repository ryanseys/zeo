even001 = ->(x) { x.even? }
p [1, 2, 3, 4].grep(even001)
even002 = ->(x) { x.even? }; p [1, 2, 3, 4].grep_v(even002)
p [1, 2, 3, 4].grep(->(x) { x.even? })
even004 = ->(x) { x.even? }
p(even004 === 4)
p [1,2,3].grep(Integer)
