# `arr[0].upcase!` and `arr[1] << "!"` change the array's own elements.
# (spinel issue #3227)
arr = ["hello", "world"]
arr[0].upcase!
arr[1] << "!"
p arr
h = { k: "value" }
h[:k].gsub!("l", "L")
p h[:k]
s = "shared"
box = ["x"]
box << s
box[1].reverse!
p s
p box
box[0].succ!
p box[0]
# hash string keys stay snapshots (CRuby dups+freezes stored keys)
k = "key"
hs = { k => 1 }
k << "-mut"
p hs.keys
p k
__END__
["HELLO", "world!"]
"vaLue"
"derahs"
["x", "derahs"]
"y"
["key"]
"key-mut"
