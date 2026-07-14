x = 5
puts(defined?(x))
puts(defined?(1 + 1))

class Foo
  def check
    @bar = 1
    puts(defined?(@bar))
  end
end
Foo.new.check
