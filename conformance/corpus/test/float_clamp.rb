def norm(a) = a.to_f.clamp(0.0, 1.0)

# receiver routed through a method param to exercise the runtime path, not a fold
def clamp_between(x, lo, hi) = x.clamp(lo, hi)

puts norm(2.5)
puts norm(-1.0)
puts norm(0.5)
puts norm(1.0)
puts norm(0.0)
puts clamp_between(7.25, 1.5, 3.5)
puts clamp_between(0.25, 1.5, 3.5)
puts clamp_between(2.5, 1.5, 3.5)
