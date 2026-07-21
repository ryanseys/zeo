p "ab cd".gsub(/\b\w/) { |c| c.upcase }
p "hello world".gsub(/\w+/) { |w| w.capitalize }
p "a1b2c3".gsub(/(\d)/) { |m| ($1.to_i * 2).to_s }
p "  x  ".gsub(/\bx\b/) { |m| "[#{m}]" }
p "aaa".sub(/a/) { |m| "X" }
