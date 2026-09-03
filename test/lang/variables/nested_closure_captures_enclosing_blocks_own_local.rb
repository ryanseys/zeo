# A name that is both an enclosing block's own local AND captured by a
# nested escaping closure must be declared once (as a shared cell), not
# also fresh-declared by the own-locals prelude.

adders = []
[1, 2, 3].each do |n|
  base = n * 100
  adders << -> { base + n }
end
p adders.map(&:call)

procs = []
[10, 20].each do |k|
  acc = 0
  procs << -> { acc }
  acc = k + 1
  procs << -> { acc }
end
p procs.map(&:call)
__END__
[101, 202, 303]
[11, 11, 21, 21]
