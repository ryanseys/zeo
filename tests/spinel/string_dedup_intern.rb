# String#-@ / #dedup interns by content: two dedups of equal contents return
# the same immortal frozen object (#2462).
p("hello".dedup.equal?("hello".dedup))
p((-"world").equal?(-"world"))
p(("he" + "llo").dedup.equal?("hello".dedup))   # computed string interns too
p("".dedup.equal?("".dedup))
p((-"a").equal?(-"b"))                            # different contents differ
p("frozen".dedup.frozen?)
p(-"frozen")
