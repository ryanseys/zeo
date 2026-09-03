# `redo` re-runs the block BODY; it does not re-yield, so the block's
# parameters and block-locals keep the values the body gave them.
n = 0
[1].each do |x; y|
  y ||= 0
  x += 10
  y += 100
  n += 1
  redo if n < 2
  p [x, y]
end

# The same rule when the redo crosses an `ensure`.
m = 0
[1].each do |v|
  v += 1
  begin
    m += 1
    redo if m < 3
  ensure
    nil
  end
  p v
end

# A `while` loop's `redo` re-runs the body without re-testing.
i = 0
k = 0
while i < 1
  k += 1
  i += 1 if k >= 2
  redo if k < 2
end
p [i, k]
__END__
[21, 200]
4
[1, 2]
