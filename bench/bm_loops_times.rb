# yjit-bench: loops-times - Integer#times performance
# Ported from https://github.com/Shopify/yjit-bench

u = 5
r = 7
a = Array.new(10000, 0)

4000.times do |i|
  4000.times do |j|
    a[i] = a[i] + j % u
  end
  a[i] = a[i] + r
end

result = a[r]
puts result
