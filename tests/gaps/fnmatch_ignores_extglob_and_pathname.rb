# `File.fnmatch` accepts every flag constant and honours only some of them.
# Two are wrong in opposite directions:
#
#   FNM_EXTGLOB    ruby expands `{a,b}` (and nests them). zeo treats the
#                  braces as literal characters, so the match FAILS where
#                  ruby succeeds.
#   FNM_PATHNAME   ruby stops `*` at a `/`, so `fnmatch("*", "a/b",
#                  FNM_PATHNAME)` is false. zeo lets `*` cross the
#                  separator, so it answers true -- a match where ruby has
#                  none, which is the more dangerous direction: a caller
#                  using it to decide "is this path inside one directory"
#                  gets yes for a nested path.
#
# The rest agree and say how narrow this is: `FNM_DOTMATCH`, `FNM_CASEFOLD`,
# `FNM_NOESCAPE`, POSIX character classes, `?`, and `FNM_PATHNAME` on a
# pattern that spells the separator itself all match ruby today.
#
# `Dir.glob` is a different implementation and does handle braces, so the two
# disagree with each other as well as with ruby.
#
# Oracle: the flags do what they say.
p File.fnmatch("{a,b}.rb", "b.rb", File::FNM_EXTGLOB)
p File.fnmatch("{a,{b,c}}.rb", "c.rb", File::FNM_EXTGLOB)
p File.fnmatch("{a,b}.rb", "b.rb")
p File.fnmatch("*", "a/b", File::FNM_PATHNAME)
p File.fnmatch("a/*", "a/b", File::FNM_PATHNAME)
p File.fnmatch("*", ".x", File::FNM_DOTMATCH)
p File.fnmatch("A*", "abc", File::FNM_CASEFOLD)
p File.fnmatch("[[:alpha:]]", "a")
p File.fnmatch("a?c", "abc")
