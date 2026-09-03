# A builtin reopen may RESTATE the class's real superclass (`class String <
# Object`); the methods attach exactly as a clauseless reopen (D3).

class String < Object
  def shout
    upcase + "!"
  end
end
puts "hi".shout
__END__
HI!
