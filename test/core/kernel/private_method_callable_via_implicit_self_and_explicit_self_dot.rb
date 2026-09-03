# Real Ruby (2.7+): a private method IS callable with an explicit
# receiver as long as it's a literal `self` -- not just implicit-self
# (no receiver at all).

class Box
  def run
    self.helper + implicit_helper
  end

  private

  def helper
    10
  end

  def implicit_helper
    helper
  end
end

puts Box.new.run
__END__
20
