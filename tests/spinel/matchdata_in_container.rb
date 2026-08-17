a001 = ["aa".match(/a/), "bb".match(/b/)]
p a001[0][0]
p a001.map { |m001| m001[0] }
h001 = { x: "zz".match(/z/) }
p h001[:x][0]
m = "hello world".match(/(\w+)\s(\w+)/)
arr = [m]
p arr[0][1]
p arr[0][2]
p arr[0].class.to_s
p arr[0].to_s
p arr.first.inspect
