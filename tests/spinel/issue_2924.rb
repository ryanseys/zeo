def k
  [5, [[1, 2, 3], [4, 5, 6]]]
end
total, chosen = k
p total
p chosen
p chosen.sum { |_u, _v, w| w }

def pair
  ["a", 7]
end
s, n = pair
p s.upcase
p n + 1
