# #692: namespaced exception class default message must use Ruby's
# `::` module separator, not the C-mangled `_` form spinel uses
# internally.

module ActiveRecord
  class RecordNotFound < StandardError
  end

  class RecordInvalid < StandardError
  end
end

module Outer
  module Inner
    class DeepError < StandardError
    end
  end
end

# Non-namespaced subclass with underscore in its name should NOT
# get the underscore replaced (My_Class is one identifier).
class My_Class < StandardError
end

puts ActiveRecord::RecordNotFound.new.message
puts ActiveRecord::RecordInvalid.new.message
puts Outer::Inner::DeepError.new.message
puts My_Class.new.message
