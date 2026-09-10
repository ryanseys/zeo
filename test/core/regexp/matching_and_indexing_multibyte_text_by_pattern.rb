# `=~` and index answer character offsets, and nil for a miss.
p("あいうえお" =~ /う/)
p "あいうえお".index(/う/)
p "abcdef".index(/d/)
p("hello" =~ /l/)
p("あいうえお" =~ /x/)
s = "日本語テスト"
p(s =~ /テ/)
p "café x".index(/x/)
__END__
2
2
3
2
nil
3
5
