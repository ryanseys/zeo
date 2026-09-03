class Box
  attr_accessor :n
end
b = Box.new
b.n = 5
b.n ||= 99
puts b.n
b.n = nil
b.n ||= 42
puts b.n
__END__
5
42
