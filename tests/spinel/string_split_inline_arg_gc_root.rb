# Inline split arguments must stay valid across compiled split calls with
# GC-managed pressure nearby. The pressure loop grows retained int arrays so
# runtime allocation and collection paths are exercised before each split.

def churn_gc
  keep = []
  while GC.stat["bytes"] <= GC.stat["threshold"] && keep.length < 64
    chunk = []
    j = 0
    while j < 65536
      chunk << j
      j = j + 1
    end
    keep << chunk
  end
  keep
end

def same_strings(a, b)
  if a.length != b.length
    return false
  end

  i = 0
  while i < a.length
    if a[i] != b[i]
      return false
    end
    i = i + 1
  end

  true
end

lines = []
i = 0
while i < 40
  lines << i.to_s
  i = i + 1
end

data = lines.join(10.chr)
keep = churn_gc

plain = data.split(10.chr)
keep = []
keep2 = churn_gc
positive = data.split(10.chr, 2)
keep2 = []
keep3 = churn_gc
negative = (data + 10.chr).split(10.chr, -1)
keep3 = []

tail = []
i = 1
while i < lines.length
  tail << lines[i]
  i = i + 1
end

expected_positive = []
expected_positive << lines[0]
expected_positive << tail.join(10.chr)

expected_negative = []
i = 0
while i < lines.length
  expected_negative << lines[i]
  i = i + 1
end
expected_negative << ""

ok = same_strings(plain, lines)
ok = ok && same_strings(positive, expected_positive)
ok = ok && same_strings(negative, expected_negative)

puts(ok ? "ok" : "bad")
