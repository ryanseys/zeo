# `attr :x` gives the reader, like attr_reader.
# (spinel issue #2952)
class C001
  attr :x
  def initialize; @x = 5; end
end
p(C001.new.x)
__END__
5
