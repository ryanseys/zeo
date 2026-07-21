n = 2500000

alt = 1.0
s0 = 0.0
s1 = 0.0
s2 = 0.0
s3 = 0.0
s4 = 0.0
s5 = 0.0
s6 = 0.0
s7 = 0.0
s8 = 0.0

d_int = 1
while d_int <= n
  d = d_int.to_f
  d2 = d * d
  d3 = d2 * d
  ds = Math.sin(d)
  dc = Math.cos(d)

  s0 = s0 + (2.0 / 3.0) ** (d - 1.0)
  s1 = s1 + 1.0 / Math.sqrt(d)
  s2 = s2 + 1.0 / (d * (d + 1.0))
  s3 = s3 + 1.0 / (d3 * ds * ds)
  s4 = s4 + 1.0 / (d3 * dc * dc)
  s5 = s5 + 1.0 / d
  s6 = s6 + 1.0 / d2
  s7 = s7 + alt / d
  s8 = s8 + alt / (2.0 * d - 1.0)

  alt = 0.0 - alt
  d_int = d_int + 1
end

puts (s0 * 1000000000).to_i
puts (s8 * 1000000000).to_i
puts "done"
