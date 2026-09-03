# `class << self` carries more than `def`: a `prepend` mixes into the singleton
# ahead of the class's own `def self.x`, an `undef_method` retires an inherited
# class method, and a `private` names one this body does not define.

module Loud
  def greet
    "LOUD #{super}"
  end
end

class Base
  def self.greet
    "base"
  end

  def self.inherited_helper
    "helper"
  end

  def self.keyword_helper
    "keyword"
  end

  def self.secret
    "secret"
  end
end

class Child < Base
  class << self
    prepend Loud

    undef_method :inherited_helper

    undef :keyword_helper

    private :secret
  end

  def self.greet
    "child"
  end
end

puts Child.greet
puts Base.greet

begin
  Child.inherited_helper
rescue NoMethodError => e
  puts "undef: #{e.class}"
end
puts Base.inherited_helper
puts Child.respond_to?(:inherited_helper)

begin
  Child.keyword_helper
rescue NoMethodError => e
  puts "undef keyword: #{e.class}"
end
puts Base.keyword_helper

begin
  Child.secret
rescue NoMethodError => e
  puts "private: #{e.class}"
end
puts Child.send(:secret)
__END__
LOUD child
base
undef: NoMethodError
helper
false
undef keyword: NoMethodError
keyword
private: NoMethodError
secret
