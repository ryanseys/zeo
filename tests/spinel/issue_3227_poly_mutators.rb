# In-place mutators through a poly (container-read) receiver: replace,
# prepend, concat, clear swap or extend the shared buffer.
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
