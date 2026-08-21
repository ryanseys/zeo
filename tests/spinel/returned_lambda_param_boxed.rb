# `lambda { }` / `proc { }` escapes from a method exactly as `->() { }` does --
# the caller has the same type-erased proc either way. Only the arrow spelling
# was marked as escaping, so the method-call spellings' parameters defaulted to
# int and a lambda returned from a method read a String argument as an Integer
# (#4035):
#
#   undefined method 'length' for an instance of Integer (NoMethodError)
def wrap_do(rules)
  lambda do |value|
    results = rules.map { |rule| rule.call(value) }
    results
  end
end

def wrap_brace(rules)
  lambda { |value| rules.map { |rule| rule.call(value) } }
end

def wrap_proc(rules)
  proc do |value|
    rules.map { |rule| rule.call(value) }
  end
end

def wrap_arrow(rules)
  ->(value) do
    rules.map { |rule| rule.call(value) }
  end
end

def wrap_return(rules)
  return lambda { |value| rules.map { |rule| rule.call(value) } }
end

len = ->(v) { v.length }
p wrap_do([len]).call("countess")
p wrap_brace([len]).call("countess")
p wrap_proc([len]).call("countess")
p wrap_arrow([len]).call("countess")
p wrap_return([len]).call("countess")

# a float argument, which an int slot would truncate rather than misread
half = ->(v) { v / 2 }
p wrap_do([half]).call(5.0)

# several parameters, and a proc's auto-splat
pair = proc { |a, b| [a, b] }
p pair.call("x", 2)
def make_pair = proc { |a, b| [a, b] }
p make_pair.call("x", 2)
p make_pair.call(["y", 3])
