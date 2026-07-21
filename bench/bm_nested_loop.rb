n = 16
x = 0
a = 0
while a < n
  b = 0
  while b < n
    c = 0
    while c < n
      d = 0
      while d < n
        e = 0
        while e < n
          f = 0
          while f < n
            x = x + 1
            f = f + 1
          end
          e = e + 1
        end
        d = d + 1
      end
      c = c + 1
    end
    b = b + 1
  end
  a = a + 1
end
puts x
puts "done"
