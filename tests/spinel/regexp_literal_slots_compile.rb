# Every literal pattern is compiled at startup into its own slot. A refusal
# there used to leave the slot NULL for the first match to dereference; it now
# reports and stops, the way CRuby refuses to run a file whose literal does not
# parse. This holds the compiling half -- the refusing half ends the process,
# so it cannot be a program that also prints.
p("aa" =~ /(?'x'a)\k'x'/)
p("ab" =~ /(?:)*/)
p("ab" =~ /(?#c)a/)
p(/a/.source)
