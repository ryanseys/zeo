# A leading local assignment inside a do...end sum block, and the same over
# Floats.
# (spinel issue #2945)
r = [1, 2, 3].sum do |t|
  x = t * 2
  x
end
p r
f = [1.0, 2.0].sum do |t|
  y = t + 1.0
  y
end
p f
__END__
12
5.0
