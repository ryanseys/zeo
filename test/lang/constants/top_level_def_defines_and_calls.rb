def greet(name, punct = "!")
  "hi #{name}#{punct}"
end
puts greet("world")
puts greet("you", "?")
__END__
hi world!
hi you?
