def fancy? = true
if fancy?
  module Helpers
    LEVEL = 3
    def self.tag = "fancy"
  end
  class Widget
    include Helpers
    def kind = "fancy-#{Helpers::LEVEL}"
  end
end
puts defined?(Helpers)
puts Helpers.tag
puts Widget.new.kind
puts Widget.ancestors.first(2).inspect
puts Object.const_defined?(:Widget)
