def matgen(n)
  a = Array.new(n * n, 0.0)
  i = 0
  while i < n
    j = 0
    while j < n
      a[i * n + j] = (1.0 / n / n) * (i - j) * (i + j)
      j = j + 1
    end
    i = i + 1
  end
  a
end

def matmul(a, b, n)
  c = Array.new(n * n, 0.0)
  i = 0
  while i < n
    k = 0
    while k < n
      aik = a[i * n + k]
      j = 0
      while j < n
        c[i * n + j] = c[i * n + j] + aik * b[k * n + j]
        j = j + 1
      end
      k = k + 1
    end
    i = i + 1
  end
  c
end

n = 150
a = matgen(n)
b = matgen(n)
c = matmul(a, b, n)

# Print checksum (integer part of first and last elements)
puts (c[0] * 1000000).to_i
puts (c[n * n - 1] * 1000000).to_i
puts "done"
