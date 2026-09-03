# Real self/ivar capture for a materialized module method -- proves
# the module body was re-typechecked against the INCLUDER's own
# concrete struct, not some shared/aliased representation.

module Counter
  def bump
    @count += 1
  end
end
class Widget
  include Counter
  def initialize
    @count = 0
  end
  def count
    @count
  end
end
w = Widget.new
w.bump
w.bump
w.bump
puts w.count
__END__
3
