puts "target.rb ran"
module Deep
  module Nest
    class Widget
      KIND = :widget
      class << self
        def kind = KIND
      end
    end
  end
end
