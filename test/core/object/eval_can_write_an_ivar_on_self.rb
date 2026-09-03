class Foo
  def initialize
    eval("@x = 42")
  end
  def x
    @x
  end
end
puts Foo.new.x
__END__
42
