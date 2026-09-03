# Frozen strings: `-@`/`#dedup` interning and the frozen-mutation guard.

# `-@` / `#dedup` intern by content: equal contents share one immortal frozen
# object.
p((-"hello").equal?(-"hello"))                 # true
p("a".dedup.equal?("a".dedup))                 # true
p(("he" + "llo").dedup.equal?("hello".dedup))  # true -- computed content interns

# `-@` on an already-frozen string returns the receiver itself.
a = "abc".freeze
p((-a).equal?(a))                              # true

# Different content interns to different objects.
p((-"x").equal?(-"y"))                         # false
p((-"frozen").frozen?)                         # true

# Byte length counts real bytes, including an embedded NUL (nothing truncates).
nul = "a\u0000b"
p nul.bytesize                                 # 3
p nul.dedup.bytesize                           # 3
p("a\u0000b".dedup.equal?("a\u0000b".dedup))   # true

# A frozen string raises on in-place mutation.
begin
  (-"immutable") << "!"
rescue FrozenError => e
  puts e.message                               # can't modify frozen String: "immutable"
end
__END__
true
true
true
true
false
true
3
3
true
can't modify frozen String: "immutable"
