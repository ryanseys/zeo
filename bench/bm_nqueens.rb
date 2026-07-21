# N-Queens solver benchmark (from yjit-bench)

def nq_solve(n)
  a = Array.new(n, -1)
  l = Array.new(n, 0)
  c = Array.new(n, 0)
  r = Array.new(n, 0)
  y0 = (1 << n) - 1
  m = 0
  k = 0

  while k >= 0
    y = (l[k] | c[k] | r[k]) & y0
    if (y ^ y0) >> (a[k] + 1) != 0
      i = a[k] + 1
      while i < n && (y & 1 << i) != 0
        i = i + 1
      end
      if k < n - 1
        z = 1 << i
        a[k] = i
        k = k + 1
        l[k] = (l[k - 1] | z) << 1
        c[k] = c[k - 1] | z
        r[k] = (r[k - 1] | z) >> 1
      else
        m = m + 1
        k = k - 1
      end
    else
      a[k] = -1
      k = k - 1
    end
  end

  m
end

total = 0
n = 0
while n < 10
  total = total + nq_solve(10)
  n = n + 1
end
puts total
puts "done"
