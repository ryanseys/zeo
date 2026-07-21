# each_char/each_line (blockless) followed by an eager block method desugars to
# chars/lines, so the block param types as String -- a multiple assignment from
# String#split inside the block resolves.
p "a,1\nb,2\n".each_line.map { |l| k, v = l.strip.split(","); k }
p "hello".each_char.map { |c| c.upcase }
p "1,2\n3,4\n".each_line.map { |l| a, b = l.strip.split(","); a.to_i + b.to_i }
p "abcde".each_char.select { |c| c > "b" }
