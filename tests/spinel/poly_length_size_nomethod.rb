# `length` / `size` on a boxed receiver that has neither: the poly length
# helper answered 0 for nil, a number and a user object, so a nil read out of
# a hash miss answered 0 where CRuby raises NoMethodError (#3974).
def probe(v)
  begin
    v.length
  rescue NoMethodError => e
    "raise: #{e.message}"
  end
end
p probe(nil)
p probe("abc")
p probe([1, 2])
p probe({ a: 1 })
p probe(5)
p probe(:sym)
def psize(v)
  begin
    v.size
  rescue NoMethodError => e
    "raise: #{e.message}"
  end
end
p psize(nil)
p psize(5)
p psize("abc")
p psize([1, 2])
