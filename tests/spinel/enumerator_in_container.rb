e = [1, 2].each
a = [e]
p a[0].to_a
e2 = [1, 2].each; h = { k: e2 }
p h[:k].to_a
e3 = [1, 2].each; [e3].each { |x| p x.to_a }
e4 = [1, 2].each; a4 = [e4]
p a4[0].next
e5 = [1, 2].each
p e5.to_a
p e5.next
n = [5]
p n[0].next
s = ["az"]
p s[0].next
p s[0].succ
