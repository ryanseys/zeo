# Each swaps or extends the shared buffer the array holds.
# (spinel issue #3227)
arr = [+"abc"]
arr[0].replace("xyz")
p arr
arr[0].prepend(">>")
p arr
arr[0].concat("!!")
p arr
arr[0].clear
p arr
arr2 = [+"axa"]
arr2[0].gsub!(/x/, "Y")
p arr2
arr2[0].sub!(/a/, "b")
p arr2
__END__
["xyz"]
[">>xyz"]
[">>xyz!!"]
[""]
["aYa"]
["bYa"]
