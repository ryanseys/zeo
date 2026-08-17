class Cell
  def initialize; @n = 1; end
  def n; @n; end
end
d001 = Cell.new.freeze
r001 = (d001.instance_variable_set(:@n, 7) rescue $!.class)
p r001
p d001.n

class Counter
  attr_accessor :m
  def initialize; @n = 1; @m = 1; end
  def n; @n; end
  def bump; @n += 1; end
  def plain; @n = @n + 1; end
end
c002 = Counter.new.freeze
r002 = (c002.bump rescue $!.class); p r002                            # => FrozenError
r003 = (c002.plain rescue $!.class); p r003                           # => FrozenError
r004 = (begin; c002.m = 5; rescue => e004; e004.class; end); p r004   # => FrozenError
p c002.n                                                              # => 1
