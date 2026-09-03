# `super()` and bare `super` mean OPPOSITE things: the parens form
# passes nothing, so the parent's optional takes its default.

class G
  def greet(name = "anon")
    "hi #{name}"
  end
end

class H < G
  def greet(name)
    super() + "!"
  end
end

puts H.new.greet("bob")
__END__
hi anon!
