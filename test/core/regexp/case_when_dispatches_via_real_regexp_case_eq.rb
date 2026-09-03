# Method wrapped in a class, not a top-level `def` -- calling a
# top-level-defined method is a separate, pre-existing, unrelated gap
# (confirmed via a plain, regex-free repro), out of scope here.

class Checker
  def check(x)
    case x
    when /^\d+$/
      "number"
    when /^[a-z]+$/
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
