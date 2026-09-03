h = { a: 1, b: 2 }
puts(h[:a])
puts(h[:b])
puts(h[:missing])
h[:c] = 3
puts(h[:c])
h[:a] = 10
puts(h[:a])
puts(h.length)
puts(h.size)
puts(h)
__END__
1
2

3
10
3
3
{a: 10, b: 2, c: 3}
