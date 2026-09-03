r = 1..5
puts(r.first)
puts(r.last)
puts(r.exclude_end?)
puts(r)

r2 = 1...5
puts(r2.exclude_end?)
puts(r2)
__END__
1
5
false
1..5
true
1...5
