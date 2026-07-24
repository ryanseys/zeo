p :hello[/(?<x>l+)/, "x"]
p :hello[/(?<x>z+)/, "x"]
p "hello"[/(?<x>l+)/, "x"]
p "hello"[/(?<y>e)/, "y"]
nm = "x"
p "hello"[/(?<x>l+)/, nm]
p :hello[/(?<x>l+)/, :x]
