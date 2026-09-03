# A bare `module_function` promotes every following `def` to a module
# method; `module_function :name` promotes an already-defined one.

module M
  module_function
  def shout(s) = s.upcase
end
module N
  def whisper(s) = s.downcase
  module_function :whisper
end
puts M.shout("hi")
puts N.whisper("HI")
__END__
HI
hi
