# A class variable (`@@x`) is shared storage owned by the class or module
# where it's first assigned. It can be declared in a module body and read
# and written from `def self.` methods.
module Counter
  @@total = 0

  def self.bump
    @@total += 1
  end

  def self.total
    @@total
  end
end

Counter.bump
Counter.bump
puts Counter.total            # 2

# `module_function` promotes methods to module methods. A bare
# `module_function` switches a mode for every following `def`; the
# `module_function :name` form promotes an already-defined one.
module Text
  module_function

  def shout(s)
    s.upcase
  end
end

module Case
  def flip(s)
    s.swapcase
  end
  module_function :flip
end

puts Text.shout("hi")         # HI
puts Case.flip("Hello")       # hELLO
__END__
2
HI
hELLO
