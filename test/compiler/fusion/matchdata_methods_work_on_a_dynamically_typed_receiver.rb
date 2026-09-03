# `$~` can be nil (whenever the last match failed), so it never infers
# as `TyKind::MatchData` and can't take codegen's static MatchData fast
# path -- it dispatches dynamically, which needs a real runtime table.
# `$~[0]` raised NoMethodError while `re.match(s)[0]` worked.

"hello" =~ /e(l+)(o)/
m = $~
p m[0]
p m[1]
p m.captures
p m.pre_match
p m.post_match
p m.to_a
p m.string
p m.to_s
__END__
"ello"
"ll"
["ll", "o"]
"h"
""
["ello", "ll", "o"]
"hello"
"ello"
