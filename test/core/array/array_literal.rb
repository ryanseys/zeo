a = [1, 2, 3]
puts(a[0])
puts(a[2])
puts(a[-1])
puts(a[10])
a[1] = 99
puts(a[1])
puts(a.length)
puts(a.size)

rest = [3, 4]
b = [1, 2, *rest, 5]
puts(b.length)
puts(b[2])
puts(b[4])
__END__
1
3
3

99
3
3
5
3
5
