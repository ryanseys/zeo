class Key
  attr_reader :k

  def initialize(k)
    @k = k
  end

  def hash
    k.hash
  end

  def eql?(other)
    other.is_a?(Key) && k == other.k
  end
end

h = {}
h[Key.new("a")] = 1
puts h[Key.new("a")]
puts h[Key.new("b")].inspect
__END__
1
nil
