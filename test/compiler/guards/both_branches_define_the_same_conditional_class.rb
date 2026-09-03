class Base
  def greet = "base"
end
def jruby? = false
if jruby?
  class Impl < Base
    def greet = "java-#{@n}"
  end
else
  class Impl < Base
    def initialize
      @n = 7
    end
    def greet = "native-#{@n}"
    def self.flavor = "self-native"
  end
end
i = Impl.new
puts i.greet
puts Impl.flavor
puts Impl.name
puts Impl.superclass
puts Impl.instance_methods(false).sort.inspect
puts i.is_a?(Base)
__END__
native-7
self-native
Impl
Base
[:greet]
true
