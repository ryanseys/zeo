p((-"hello").equal?(-"hello"))
p("a".dedup.equal?("a".dedup))
p(("he" + "llo").dedup.equal?("hello".dedup))
a = "abc".freeze
p((-a).equal?(a))
p((-"x").equal?(-"y"))
p((-"frozen").frozen?)
nul = "a b"
p nul.bytesize
p("a b".dedup.equal?("a b".dedup))
begin
  (-"immutable") << "!"
rescue FrozenError => e
  puts e.message
end
__END__
true
true
true
true
false
true
3
true
can't modify frozen String: "immutable"
