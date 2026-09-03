module Sayer
  def say; "said"; end
end
module Speaker; end
Speaker.extend(Sayer)
puts Speaker.say
puts Speaker.respond_to?(:say)
__END__
said
true
