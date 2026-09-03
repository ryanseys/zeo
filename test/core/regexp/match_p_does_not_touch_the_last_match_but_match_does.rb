# `match?` is specifically the allocation-free predicate: it builds no
# MatchData and so leaves the slot alone. `Regexp#match` sets it.

"abc" =~ /b/
p $&
"xyz".match?(/y/)
p $&
/(\d)/.match("a1")
p $1
__END__
"b"
"b"
"1"
