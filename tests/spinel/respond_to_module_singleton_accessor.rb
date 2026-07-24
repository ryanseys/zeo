# class << self; attr_accessor :x on a module must be visible to
# respond_to? for both the reader and the writer.
module Foo
  class << self
    attr_accessor :x
  end
end
puts Foo.respond_to?(:x)
puts Foo.respond_to?(:x=)
puts Foo.respond_to?(:nope)
