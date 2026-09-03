count = 100
3.times do
  count += 1
end
$lib_count = count
class CounterBox
  def initialize
    @n = 0
  end
  def bump
    @n += 1
  end
  def n
    @n
  end
end
