# `alias $name $&` (aliasing a plain global to one of the special
# regex-match globals like $&, $', $1) isn't lowered -- zeo rejects it at
# compile time. This breaks bare `require "English"`, which defines all its
# readable names (`$MATCH`, `$PREMATCH`, etc) this way.
alias $MATCH $&
"hello" =~ /ell/
p $MATCH
