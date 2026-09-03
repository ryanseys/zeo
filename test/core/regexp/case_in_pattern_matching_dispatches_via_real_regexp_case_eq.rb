class Checker
  def check(x)
    case x
    in /^\d+$/
      "number"
    in /^[a-z]+$/
      "lower"
    else
      "other"
    end
  end
end
c = Checker.new
puts c.check("123")
puts c.check("abc")
puts c.check("ABC")
__END__
number
lower
other
