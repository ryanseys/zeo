# The named group's text, nil when the pattern misses, and a group name read
# from a local.
p :hello[/(?<x>l+)/, "x"]
p :hello[/(?<x>z+)/, "x"]
p "hello"[/(?<x>l+)/, "x"]
p "hello"[/(?<y>e)/, "y"]
nm = "x"
p "hello"[/(?<x>l+)/, nm]
p :hello[/(?<x>l+)/, :x]
__END__
"ll"
nil
"ll"
"e"
"ll"
"ll"
