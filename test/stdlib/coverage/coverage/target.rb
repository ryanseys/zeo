def cov_add(a, b)
  a + b
end

def cov_never(x)
  x * 100
end

class CovCounter
  def initialize(start)
    @n = start
  end

  def bump
    @n += 1
    @n
  end

  def big?
    if @n > 10
      true
    else
      false
    end
  end
end

COV_LIMIT = 3

COV_LIMIT.times do |i|
  cov_add(i, i)
end
