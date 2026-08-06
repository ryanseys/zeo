# `method_missing` defined as a CLASS method is never consulted: zeo raises
# NoMethodError for the missing class method instead of routing to it. Both
# spellings are affected -- `def self.method_missing` and a `method_missing`
# inside `class << self`.
#
# This is the whole public API of some gems. Faker's is one call away from it:
#
#   Faker::Name.first_name   # => Faker::Base.method_missing(:first_name)
#
# Ruby prints the two "flex" lines below.

class Base
  class << self
    def method_missing(name, *args)
      return "flex #{name}" if name.to_s.start_with?("m_")
      super
    end

    def respond_to_missing?(name, include_private = false)
      name.to_s.start_with?("m_") || super
    end
  end
end

class Plain
  def self.method_missing(name, *args)
    return "flex2 #{name}" if name.to_s.start_with?("m_")
    super
  end
end

p Base.m_colour
p Plain.m_size
p Base.respond_to?(:m_colour)
