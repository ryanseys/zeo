p([].lazy.select { |x| true }.first(2))
p([].lazy.reject { |x| false }.to_a)
p([1, 2].lazy.select { |x| true }.first(2))
