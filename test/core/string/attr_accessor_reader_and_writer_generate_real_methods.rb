class Box
  attr_accessor :value
  attr_reader :ro
  def initialize
    @value = 1
    @ro = 7
  end
end
b = Box.new
puts b.value
b.value = 99
puts b.value
puts b.ro
__END__
1
99
7
