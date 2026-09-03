# A `def` in expression position (a `Class.new` block) threads its
# block too.

k = Class.new do
  def run
    yield 10
  end
end
puts k.new.run { |x| x * 2 }
__END__
20
