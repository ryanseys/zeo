# Real Ruby: unlike an ordinary Proc/block, `return`/`break` inside a
# lambda act like a method boundary -- they terminate just the lambda
# call, never the enclosing method.

class Runner
  def return_test
    f = -> {
      return 10
      20
    }
    puts f.call
    "after"
  end

  def break_test
    g = -> {
      break 99
      100
    }
    puts g.call
    "after"
  end
end

r = Runner.new
puts r.return_test
puts r.break_test
__END__
10
after
99
after
