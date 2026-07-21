# Top-level \D \W \S must match the complement set (not double-negate).
p("5" =~ /\D/); p("x" =~ /\D/)
p("a" =~ /\W/); p(" " =~ /\W/)
p("x" =~ /\S/); p(" " =~ /\S/)
# Non-negated and bracket forms unchanged.
p("5" =~ /\d/); p("x" =~ /\d/)
p("a" =~ /\w/); p(" " =~ /\w/)
p("5" =~ /[\D]/); p("x" =~ /[\D]/)
p("héllo" =~ /\D/)
