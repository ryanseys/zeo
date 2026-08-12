a = [1, 2, 3]
seen = []
for x in a
  seen << x
  a << x + 10 if x < 3      # append during iteration
end
p seen
b = [1, 2, 3, 4]
seen2 = []
for y in b
  seen2 << y
  b.pop if y == 1           # shrink during iteration
end
p seen2
c = [1, 2, 3]
seen3 = []
for z in c
  seen3 << z
  c[1] = 99 if z == 1       # rewrite upcoming slot
end
p seen3
