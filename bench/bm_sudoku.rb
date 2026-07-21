# Sudoku solver benchmark (from yjit-bench)
# Adapted for Spinel AOT compilation
# Multi-return encoded: forward returns min * 1000 + min_c

def sd_genmat_mr
  mr = Array.new(324 * 9, 0)
  mr_len = Array.new(324, 0)
  mc = Array.new(729 * 4, 0)
  r = 0
  i = 0
  while i < 9
    j = 0
    while j < 9
      k = 0
      while k < 9
        mc[r * 4] = 9 * i + j
        mc[r * 4 + 1] = (i / 3 * 3 + j / 3) * 9 + k + 81
        mc[r * 4 + 2] = 9 * i + k + 162
        mc[r * 4 + 3] = 9 * j + k + 243
        r = r + 1
        k = k + 1
      end
      j = j + 1
    end
    i = i + 1
  end
  r2 = 0
  while r2 < 729
    c2 = 0
    while c2 < 4
      idx = mc[r2 * 4 + c2]
      mr[idx * 9 + mr_len[idx]] = r2
      mr_len[idx] = mr_len[idx] + 1
      c2 = c2 + 1
    end
    r2 = r2 + 1
  end
  mr
end

def sd_genmat_mc
  mc = Array.new(729 * 4, 0)
  r = 0
  i = 0
  while i < 9
    j = 0
    while j < 9
      k = 0
      while k < 9
        mc[r * 4] = 9 * i + j
        mc[r * 4 + 1] = (i / 3 * 3 + j / 3) * 9 + k + 81
        mc[r * 4 + 2] = 9 * i + k + 162
        mc[r * 4 + 3] = 9 * j + k + 243
        r = r + 1
        k = k + 1
      end
      j = j + 1
    end
    i = i + 1
  end
  mc
end

# Returns min * 1000 + min_c (encoded pair)
def sd_update_forward(mr, mc, sr, sc, r)
  min = 10
  min_c = 0
  c2 = 0
  while c2 < 4
    idx = mc[r * 4 + c2]
    sc[idx] = sc[idx] + 128
    c2 = c2 + 1
  end
  c2 = 0
  while c2 < 4
    base = mc[r * 4 + c2] * 9
    r2 = 0
    while r2 < 9
      rr = mr[base + r2]
      sr[rr] = sr[rr] + 1
      if sr[rr] == 1
        cc2 = 0
        while cc2 < 4
          cc = mc[rr * 4 + cc2]
          sc[cc] = sc[cc] - 1
          if sc[cc] < min
            min = sc[cc]
            min_c = cc
          end
          cc2 = cc2 + 1
        end
      end
      r2 = r2 + 1
    end
    c2 = c2 + 1
  end
  min * 1000 + min_c
end

def sd_update_reverse(mr, mc, sr, sc, r)
  c2 = 0
  while c2 < 4
    idx = mc[r * 4 + c2]
    sc[idx] = sc[idx] - 128
    c2 = c2 + 1
  end
  c2 = 0
  while c2 < 4
    base = mc[r * 4 + c2] * 9
    r2 = 0
    while r2 < 9
      rr = mr[base + r2]
      sr[rr] = sr[rr] - 1
      if sr[rr] == 0
        sc[mc[rr * 4]] = sc[mc[rr * 4]] + 1
        sc[mc[rr * 4 + 1]] = sc[mc[rr * 4 + 1]] + 1
        sc[mc[rr * 4 + 2]] = sc[mc[rr * 4 + 2]] + 1
        sc[mc[rr * 4 + 3]] = sc[mc[rr * 4 + 3]] + 1
      end
      r2 = r2 + 1
    end
    c2 = c2 + 1
  end
end

def sd_solve(mr, mc, s)
  sr = Array.new(729, 0)
  sc = Array.new(324, 9)
  hints = 0
  i = 0
  while i < 81
    a = s[i] - 49
    if a >= 0 && a < 9
      sd_update_forward(mr, mc, sr, sc, i * 9 + a)
      hints = hints + 1
    end
    i = i + 1
  end

  cr = Array.new(81, -1)
  cc = Array.new(81, 0)
  i = 0
  min = 10
  dir = 1
  count = 0

  while true
    while i >= 0 && i < 81 - hints
      if dir == 1
        if min > 1
          c = 0
          while c < 324
            if sc[c] < min
              min = sc[c]
              cc[i] = c
              if min < 2
                c = 324
              end
            end
            c = c + 1
          end
        end
        if min == 0 || min == 10
          cr[i] = -1
          dir = -1
          i = i - 1
          next
        end
      end

      c = cc[i]
      if dir == -1 && cr[i] >= 0
        sd_update_reverse(mr, mc, sr, sc, mr[c * 9 + cr[i]])
      end

      r2 = cr[i] + 1
      while r2 < 9 && sr[mr[c * 9 + r2]] != 0
        r2 = r2 + 1
      end

      if r2 < 9
        encoded = sd_update_forward(mr, mc, sr, sc, mr[c * 9 + r2])
        min = encoded / 1000
        cc[i + 1] = encoded - min * 1000
        cr[i] = r2
        dir = 1
        i = i + 1
      else
        cr[i] = -1
        dir = -1
        i = i - 1
      end
    end

    if i < 0
      break
    end
    count = count + 1
    i = i - 1
    dir = -1
  end
  count
end

mr = sd_genmat_mr
mc = sd_genmat_mc

puzzle = [46,46,46,46,46,46,46,46,46,46,46,46,46,46,51,46,56,53,46,46,49,46,50,46,46,46,46,46,46,46,53,46,55,46,46,46,46,46,52,46,46,46,49,46,46,46,57,46,46,46,46,46,46,46,53,46,46,46,46,46,46,55,51,46,46,50,46,49,46,46,46,46,46,46,46,46,52,46,46,46,57]

total = 0
n = 0
while n < 100
  total = total + sd_solve(mr, mc, puzzle)
  n = n + 1
end
puts total
puts "done"
