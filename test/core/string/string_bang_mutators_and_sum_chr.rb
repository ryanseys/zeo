s = "abc"; p s.upcase!; p s; p s.upcase!
t = "  hi  "; t.strip!; p t
u = "hello"; u.reverse!; p u
v = "a b c"; v.gsub!(" ", "-"); p v
w = "az"; w.succ!; p w
p "hello".sum
p "hello".chr
__END__
"ABC"
"ABC"
nil
"hi"
"olleh"
"a-b-c"
"ba"
532
"h"
