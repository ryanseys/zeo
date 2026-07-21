# begin/end/first/last/min/max on a String-endpoint Range read the endpoints
# as strings (the int-backed sp_Range stores their pointers), and first(n)/
# last(n) materialize n elements via string succ.
p(("a".."e").begin)
p(("a".."e").end)
p(("a".."e").first)
p(("a".."e").last)
p(("a".."e").min)
p(("a".."e").max)
p(("a".."e").first(2))
p(("a".."e").last(2))
r = ("b".."d")
p(r.begin)
p(r.max)
