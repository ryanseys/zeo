# A namespaced exception class's default message names the class with Ruby's
# `::` separator, whatever spelling the compiler uses for the symbol behind
# it.

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
__END__
ActiveRecord::RecordNotFound
ActiveRecord::RecordInvalid
Outer::Inner::DeepError
My_Class
