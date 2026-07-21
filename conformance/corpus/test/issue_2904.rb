p [[1, 2], [3, 4]].map { |row| row.find { |n| n > 2 } }
p [["a", "bb"], ["ccc"]].map { |row| row.find { |s| s.length > 1 } }
p [[1, 2], [3, 4]].map { |row| row.detect { |n| n > 5 } }
