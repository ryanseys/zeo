def f(s)
  s =~ /(l)/
  $1
end
p f("hello")
"abc" =~ /(b)/
p f("hello")
p $1

def g(s)
  s.scan(/(\d)/)
  1
end
"abc" =~ /(b)/
g("x1")
p $1

def h(s); s.gsub(/(o)/) { "0" }; end
"abc" =~ /(b)/
h("foo")
p $1

def blk
  [1].each { "zz" =~ /(z)/ }
  $1
end
"abc" =~ /(b)/
p blk
p $1

def early(s)
  return "no" unless s =~ /(x)/
  $1
end
"abc" =~ /(b)/
p early("zz")
p $1
p early("xy")
p $1

def outer(s)
  s =~ /(o)/
  inner("zzz")
  $1
end
def inner(s); s =~ /(z)/; $1; end
p outer("foo")
p $~ && $~[0]
