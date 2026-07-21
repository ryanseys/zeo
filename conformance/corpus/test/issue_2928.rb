p [[3, 1, 2], [6, 4, 5]].map(&:sort)
p({ a: [3, 1], b: [2, 4] }.map { |k, v| v.sort })
p [[9, 7, 8]].map { |row| row.sort }
