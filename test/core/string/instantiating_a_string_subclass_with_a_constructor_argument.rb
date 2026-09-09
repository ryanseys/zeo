# `Class.new(String).new("x")` forwards the argument to String's initializer.
klass = Class.new(String) do
  def foo
    42
  end
end
s = klass.new("x")
p s
p s.foo
__END__
"x"
42
