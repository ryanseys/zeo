# Integer#[start, len]: a negative start shifts left (5[-1,3] == 2), a
# negative len keeps the whole shifted value with no mask (5[2,-1] == 1,
# 255[0,-5] == 255), a zero len selects nothing, and a positive len masks.

p 0b1011010[1, 3]
p 255[0, 4]
p 5[2, -1]
p 5[-1, 3]
p 255[0, -5]
p 7[0, 0]
p(-1[60, 8])
__END__
5
15
1
2
255
0
255
