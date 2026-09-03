# Forwards the CURRENT bindings (the reassigned `name`), through
# required and optional params alike.

class A
  def greet(name, punct = ".")
    "hi #{name}#{punct}"
  end
end

class B < A
  def greet(name, punct = ".")
    name = name + "!"
    super
  end
end

puts B.new.greet("bob")
puts B.new.greet("bob", "?")
__END__
hi bob!.
hi bob!?
