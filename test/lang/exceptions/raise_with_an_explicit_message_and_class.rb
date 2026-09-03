class MyError < StandardError
end
class Risky
  def check(n)
    raise MyError, "bad value: #{n}" if n < 0
    n * 2
  end
end
r = Risky.new
puts r.check(5)
puts r.check(-1)
__END__
10
#@ stderr
lang/exceptions/raise_with_an_explicit_message_and_class.rb:5:in 'Risky#check': bad value: -1 (MyError)
	from lang/exceptions/raise_with_an_explicit_message_and_class.rb:11:in '<main>'
#@ exit 1
