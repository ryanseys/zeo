[[:x, :y]].each { |e| p e.to_s }
x = [[:x, :y]]
p x[0].to_s
p [[:a], [:b, :c]].map(&:to_s)
p [[1, 2], [:s]].map(&:to_s)
p [[:x, :y]].to_s
