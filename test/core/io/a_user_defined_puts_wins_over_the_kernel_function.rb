# A user-defined sibling `puts` WINS over the Kernel function (real Ruby's
# rule).

class Logger
  def puts(msg)
    $stdout_lines = 1
    "logged: #{msg}"
  end
  def run
    puts("hi")
  end
end

v = Logger.new.run
p v
__END__
"logged: hi"
