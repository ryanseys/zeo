# Both endpoints answer, for two ranges in one array.
# (spinel issue #2938)
ranges = [(1..5), (10..20)]
p ranges[0].begin + ranges[0].end
p ranges[1].begin
p ranges[1].end
__END__
6
10
20
