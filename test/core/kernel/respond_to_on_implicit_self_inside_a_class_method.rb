# Implicit-self respond_to? inside a `def self.x` resolves against the
# class value (like explicit self.respond_to?), so a sibling class method
# answers true, not false.

class Screen
  def self.build
    puts respond_to?(:build)
    puts respond_to?(:name)
    puts respond_to?(:nope_xyz)
  end
  def self.other; end
end
Screen.build
__END__
true
true
false
