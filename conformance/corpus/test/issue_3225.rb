p (1..8).chunk { |x| x.even? }.to_a
p (1..5).chunk { |x| x % 3 }.to_a
p (1..6).chunk { |x| x < 4 }.map { |k, xs| [k, xs.sum] }
