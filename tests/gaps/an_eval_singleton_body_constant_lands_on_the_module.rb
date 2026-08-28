src = <<~SRC
  module Schemes
    class << self
      CHARS = ".+-"
      def list = constants
    end
  end
SRC
eval src, nil, "s.rb"
p Schemes.list
p Schemes.constants
p Schemes.singleton_class.constants(false)
