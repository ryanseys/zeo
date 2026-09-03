# ^/$ do not match at the phantom position after a final newline
p("a\n".scan(/^/).size)
p("a\nb\n".scan(/^/).size)
p(/^$/.match?("a\n"))
p("x\n".match?(/x$/))
# inline (?m:) is Ruby DOTALL (dot spans newline), scoped to the group
p(/(?m:a.c)/ =~ "a\nc")
p(/a.c/ =~ "a\nc")
p(/(?i-m:a.b)/ =~ "A\nB")
# absence operator: match only text NOT containing "foo"
p(/\A(?~foo)\z/.match?("bar"))
p(/\A(?~foo)\z/.match?("xfooy"))
# alternation captures land in the right slots
p(/(a)|(b)|(c)/.match("c").captures)
# onig accepts a redundant nested repeat the Rust engines reject
p(Regexp.new("a***").match?("aaa"))
__END__
1
2
false
true
0
nil
nil
true
false
[nil, nil, "c"]
true
#@ stderr
gaps/oniguruma_backed_semantics_match_the_oracle.rb:16: warning: regular expression has redundant nested repeat operator '*': /a***/
gaps/oniguruma_backed_semantics_match_the_oracle.rb:16: warning: regular expression has redundant nested repeat operator '*': /a***/
