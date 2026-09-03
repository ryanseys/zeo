# `s == s` must not deadlock: comparing a struct to itself used to lock the
# same slots Mutex twice and hang. `keyword_init?` is tri-state, matching
# CRuby -- `nil` when never specified, else the boolean passed.

S = Struct.new(:a, :b)
s = S.new(1, 2)
p(s == s)
p(s == S.new(1, 2))
p(s == S.new(1, 9))
p S.keyword_init?
p Struct.new(:a, keyword_init: true).keyword_init?
p Struct.new(:a, keyword_init: false).keyword_init?
__END__
true
true
false
nil
true
false
