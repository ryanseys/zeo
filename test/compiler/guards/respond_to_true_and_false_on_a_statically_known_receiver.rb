class Dog
  def bark
    "woof"
  end
end

d = Dog.new
puts d.respond_to?(:bark)
puts d.respond_to?(:meow)
__END__
true
false
