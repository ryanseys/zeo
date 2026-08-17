# A keyword hash that no declared keyword parameter consumes degrades to one
# positional hash at the tail of a *rest. The poly-dispatch arm did not do
# that, so a call through a polymorphic receiver ran with no arguments at all
# (#3528, the #3503 shape one call path further out).
class Rel
  def initialize
    @parts = []
  end

  def order(*parts)
    @parts.concat(parts)
    self
  end

  def parts
    @parts
  end
end

class Other
  def order(*parts)
    self
  end
end

def take(r)
  r.order(id: :desc)
end

things = [Rel.new, Other.new]
p take(things[0]).parts

# positional and keyword together, on a fresh receiver so the parts are this
# call's alone
def take2(r)
  r.order(:a, id: :desc)
end
fresh = [Rel.new, Other.new]
p take2(fresh[0]).parts

# a declared keyword param still consumes it rather than degrading
class Kw
  def initialize
    @seen = nil
  end

  def order(*parts, id: :none)
    @seen = [parts, id]
    self
  end

  def seen
    @seen
  end
end

def take3(r)
  r.order(id: :desc)
end
kws = [Kw.new, Kw.new]
p take3(kws[0]).seen
