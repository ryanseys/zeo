# The count form of min_by/max_by over a Range: the Range has to be
# materialized before it can be indexed (#3860). MatchData#values_at with no
# arguments selects nothing (#3846).
p((1..5).min_by(2) { |x| -x })
p((1..5).max_by(2) { |x| -x })
p((1..5).min_by { |x| -x })
p([3, 1, 2].min_by(2) { |x| x })
p("hello".match(/(l)(o)/).values_at)
p("hello".match(/(l)(o)/).values_at(1, 2))
