# `ai.zip(tj) { }` where both rows are elements of a poly table: neither side
# is a statically typed array. The block form handed the sp_RbVal straight to
# the typed array accessor (invalid C), and with a poly RECEIVER the call fell
# through to the runtime dispatch, which has no zip arm and raised.

def mat(n)
  rows = []
  n.times do |i|
    row = []
    n.times { |j| row << (i + j).to_f }
    rows << row
  end
  rows
end

def mul(a, t)
  c = []
  a.each_with_index do |ai, i|
    ci = []
    t.each_with_index do |tj, j|
      s = 0.0
      ai.zip(tj) do |av, tv|
        s += av * tv
      end
      ci[j] = s
    end
    c << ci
  end
  c
end

m = mat(3)
r = mul(m, m)
p r[0][0]
p r[2][2]
