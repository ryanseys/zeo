# A patch calling another patch (implicit self -> direct free-fn call, the
# result's `+` going back through dynamic String dispatch), and an unknown
# method still raising real Ruby's exact NoMethodError.

class String
  def shout
    exclaim + "?"
  end

  def exclaim
    self + "!"
  end
end

puts "hey".shout
begin
  "hey".nope
rescue NoMethodError => e
  puts e.message
end
__END__
hey!?
undefined method 'nope' for an instance of String
