# The block sees each match, and $1 inside it, for word-boundary, digit and
# whole-word patterns.
# (spinel issue #2910)
p "ab cd".gsub(/\b\w/) { |c| c.upcase }
p "hello world".gsub(/\w+/) { |w| w.capitalize }
p "a1b2c3".gsub(/(\d)/) { |m| ($1.to_i * 2).to_s }
p "  x  ".gsub(/\bx\b/) { |m| "[#{m}]" }
p "aaa".sub(/a/) { |m| "X" }
__END__
"Ab Cd"
"Hello World"
"a2b4c6"
"  [x]  "
"Xaa"
