# Two nested each_with_index loops filling a result array from zip and sum.
a = [[1, 2, 3], [4, 5, 6]]
b = [[7, 8], [9, 10], [11, 12]]
result = Array.new(a.size) { Array.new(b.first.size, 0) }
a.each_with_index do |row, i|
  b.transpose.each_with_index do |col, j|
    result[i][j] = row.zip(col).sum { |x, y| x * y }
  end
end
p result
__END__
[[58, 64], [139, 154]]
