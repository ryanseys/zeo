p([1, nil, 2].lazy.compact.to_a)
p([1, 2, 3].lazy.zip([4, 5, 6]).to_a)
p([1, 2, 3].lazy.map { |x| x * 2 }.to_a)
