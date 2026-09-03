# `File.fnmatch` honours every flag it accepts.
#
# It used to accept all five and honour one (`FNM_CASEFOLD`), which made
# two of them wrong in opposite directions: `FNM_EXTGLOB` treated `{a,b}`
# as literal characters, so a match FAILED where ruby succeeds, and
# `FNM_PATHNAME` let `*` cross a `/`, so it answered true where ruby has
# no match -- the more dangerous direction, since a caller using it to ask
# "is this path inside one directory" got yes for a nested path.
#
# Rewriting it against CRuby's own rules closed four more the file never
# named: the leading-DOT rule (`fnmatch("*", ".x")` is false without
# `FNM_DOTMATCH`), backslash escapes (`FNM_NOESCAPE` turns them off),
# bracket RANGES (`[a-c]`), and a `]` in the first position of a set,
# which ends the set in `fnmatch` where glob reads it as a member.

p File.fnmatch("{a,b}.rb", "b.rb", File::FNM_EXTGLOB)
p File.fnmatch("{a,{b,c}}.rb", "c.rb", File::FNM_EXTGLOB)
p File.fnmatch("{a,b}.rb", "b.rb")
p File.fnmatch("*", "a/b", File::FNM_PATHNAME)
p File.fnmatch("a/*", "a/b", File::FNM_PATHNAME)
p File.fnmatch("*", ".x", File::FNM_DOTMATCH)
p File.fnmatch("A*", "abc", File::FNM_CASEFOLD)
p File.fnmatch("[[:alpha:]]", "a")
p File.fnmatch("a?c", "abc")
__END__
true
true
false
false
true
true
true
false
true
