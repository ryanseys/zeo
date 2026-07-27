# A method whose body ends in a bare `raise` never returns through that
# statement, so the raise must not contribute to the return type. Unifying its
# void with the explicit returns' Integer has no rule and lands on poly, which
# boxes the return of every `return x if cond; raise` guard method and, through
# its callers, whatever they feed.
class Table
  def initialize(names) = @names = names

  def offsets
    h = {}
    i = 0
    while i < @names.length
      h[@names[i]] = i * 2
      i += 1
    end
    h
  end

  # the shape: two guarded returns, then a raise
  def lookup(token, offs)
    return offs[token] if offs.key?(token)
    return token.to_i if token != ""
    raise ArgumentError, "unknown token"
  end
end

t = Table.new(["a", "b", "c"])
o = t.offsets
p o.keys.sort
p o["c"]
p t.lookup("b", o)
p t.lookup("41", o)
p t.lookup("a", o) + t.lookup("1", o)
begin
  t.lookup("", o)
rescue ArgumentError => e
  puts e.message
end
