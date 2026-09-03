# The regexp engine is linked: compilation, named captures and unicode
# classes all work in the binary.
m = "2026-09-02".match(/(?<y>\d{4})-(?<m>\d{2})-(?<d>\d{2})/)
puts m[:y], m[:m], m[:d]
p "café niño".scan(/\p{L}+/)
p "a1b22c333".gsub(/\d+/) { |d| "<#{d.length}>" }
__END__
2026
09
02
["café", "niño"]
"a<1>b<2>c<3>"
