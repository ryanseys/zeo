# A blockless each_line or each_char followed by map or select, with a multiple
# assignment from split inside the block.
# (spinel issue #2901)
p "a,1\nb,2\n".each_line.map { |l| k, v = l.strip.split(","); k }
p "hello".each_char.map { |c| c.upcase }
p "1,2\n3,4\n".each_line.map { |l| a, b = l.strip.split(","); a.to_i + b.to_i }
p "abcde".each_char.select { |c| c > "b" }
__END__
["a", "b"]
["H", "E", "L", "L", "O"]
[3, 7]
["c", "d", "e"]
