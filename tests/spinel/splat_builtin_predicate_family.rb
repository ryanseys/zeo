# the predicate/search builtins take their argument as a scalar, so an
# unexpanded splat answered from a comparison against the array pointer
x001 = ["b"]
p("abc".include?(*x001))

x002 = ["a"]
p("abc".start_with?(*x002))

x003 = ["c"]
p("abc".end_with?(*x003))

x004 = ["b"]
p("abc".index(*x004))

x005 = ["b"]
p("abc".count(*x005))

x006 = ["-"]
p("a-b".split(*x006))

x007 = ["-"]
p([1, 2].join(*x007))

x008 = [1]
p({ a: 1 }.value?(*x008))

x009 = ["a", "z"]
p("abc".tr(*x009))

# the key predicates aborted the compile: the splat array reached the slot
# expecting one key
k001 = [:a]
p({ a: 1 }.key?(*k001))

k002 = [:a]
p({ a: 1 }.has_key?(*k002))

k003 = [:a]
p({ a: 1 }.include?(*k003))

k004 = [:a]
p({ a: 1 }.member?(*k004))

x010 = [3]
p([1, 2, 3].include?(*x010))

x011 = [2]
p([1, 2, 3].index(*x011))

x012 = ["b"]
p("abc".rindex(*x012))

# a length only the run time knows: the splat arrives as a parameter
def f005(keys)
  { a: 1 }.fetch(*keys)
end
p f005([:a])

def f006(keys)
  h = { a: 1 }
  h.delete(*keys)
  h
end
p f006([:a])

def f007(keys)
  { a: 1 }[*keys]
end
p f007([:a])

def f008(idx)
  [1, 2].fetch(*idx)
end
p f008([0])

def f009(idx)
  [1, 2][*idx]
end
p f009([0])

def f010(args)
  "abc".sub(*args)
end
p f010(["a", "z"])

def f011(args)
  "abc".delete(*args)
end
p f011(["b"])

def f012(sep)
  "a-b-c".split(*sep)
end
p f012(["-"])

def f013(needle)
  "abc".include?(*needle)
end
p f013(["b"])

def f014(keys)
  { a: 1 }.key?(*keys)
end
p f014([:a])

# a user method of the same name still reads the array into its own params
class Bag
  def initialize(items)
    @items = items
  end

  def index(a, b)
    a + b
  end
end
p Bag.new([1]).index(*[3, 4])

# the variadic builtins keep taking the array as it stands
p [1, 2, 3].values_at(*[0, 2])
p [1, 2, 3].dig(*[0])
