# #192 feature (1): a `super` in an override installed via a `Class.new(Parent)`
# block reaches the parent's method, when the parent is a runtime class. The
# override forwards args and composes the parent's result.

base = Class.new do
  def greet(n)
    "hi #{n}"
  end
end
sub = Class.new(base) do
  def greet(n)
    super(n) + "!"
  end
end
p sub.new.greet("x")
__END__
"hi x!"
