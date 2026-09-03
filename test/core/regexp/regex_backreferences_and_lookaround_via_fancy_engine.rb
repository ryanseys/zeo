# Constructs the linear-time `regex` crate can't do; a backtracking engine
# transparently backs them. The fast path stays unaffected.

puts("hello" =~ /(\w)\1/)             # 2
p("abc" =~ /(\w)\1/)                  # nil
puts "foobar".match?(/foo(?=bar)/)    # true
puts "foobaz".match?(/foo(?=bar)/)    # false
puts "$100".gsub(/(?<=\$)\d+/, "N")   # $N
puts "catfish".match?(/cat(?!fish)/)  # false
puts "abc".match?(/a(?#c)bc/)         # true
puts "book".match?(/(?<c>o)\k<c>/)    # true
# fast path unaffected
m = "2024-01-15".match(/(\d+)-(\d+)-(\d+)/)
puts m[2]                             # 01
p "a,b,c".split(/,/)                  # ["a", "b", "c"]
__END__
2
nil
true
false
$N
false
true
true
01
["a", "b", "c"]
