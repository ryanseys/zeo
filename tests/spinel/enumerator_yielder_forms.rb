p(Enumerator.new { |y| y << 1 << 2 }.to_a)
p(Enumerator.new { |y| y << "a" << "b" }.to_a)
p(Enumerator.new { |y| (y << 1) << 2 }.to_a)
p(Enumerator.new { |y| y << 1 << 2 << 3 }.to_a)
p(Enumerator.new { |y| [1, 2, 3].each(&y) }.to_a)
p(Enumerator.new { |y| [1, 2].map(&y) }.to_a)
p(Enumerator.new { |y| (1..3).each(&y) }.first(2))
e = Enumerator.new { |y| [10, 20].each(&y) }
p e.next
p e.next
