class Thing
  attr_reader :x
  def set_both(g)
    @x, $g2 = 10, g
  end
end
t = Thing.new
t.set_both(99)
puts t.x
puts $g2
__END__
10
99
