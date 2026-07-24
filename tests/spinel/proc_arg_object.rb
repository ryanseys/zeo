class Wrapper
  def initialize(val)
    @val = val
    @extra = []  # force heap allocation (pointer ivar)
  end

  def value
    @val
  end
end

def pass(&block)
  w = Wrapper.new(77)
  block.call(w)
end

pass { |w| puts w.value }
