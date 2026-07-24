class Router
  def self.match_pattern(s)
    return nil if s == ""
    h = {}
    h["k"] = s
    h
  end
end

r = Router.match_pattern("hello")
if r
  puts r["k"]
end

r2 = Router.match_pattern("")
if r2
  puts "yes"
else
  puts "no"
end

# Same shape, nilable str_int_hash return
def maybe_count(n)
  return nil if n <= 0
  h = {}
  h["x"] = n
  h
end

c = maybe_count(5)
if c
  puts c["x"]
end
