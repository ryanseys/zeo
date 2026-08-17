a1 = [1, 2]
p(begin; a1["x"] = 9; rescue TypeError => e; e.message; end)
p a1

a2 = [1, 2]
p(begin; a2[:s] = 9; rescue TypeError => e; e.message; end)
p a2

a3 = [1, 2]
p(begin; a3[nil] = 9; rescue TypeError => e; e.message; end)
p a3

a4 = [1, 2]
p(begin; a4[[0]] = 9; rescue TypeError => e; e.message; end)

a5 = ["x"]
p(begin; a5["y"] = "z"; rescue TypeError => e; e.message; end)
p a5

a6 = [1, 2]
i = "x"
p(begin; a6[i] = 9; rescue TypeError => e; e.message; end)

a7 = [1, 2]
a7[0] = 9
a7[1.5] = 8
p a7

b = [1, 2]
p(begin; b["x"] = 9; rescue TypeError => e; e.message; end)
b = ["z"]
p b

c = [1, 2]
p(begin; c[:s] = 9; rescue TypeError => e; e.message; end)
c = ["z"]
p c

d = [1, 2]
p(begin; d[nil] = 9; rescue TypeError => e; e.message; end)
d = ["z"]
p d

f = [1, 2]
f[1.5] = 7
f = ["z"]
p f
