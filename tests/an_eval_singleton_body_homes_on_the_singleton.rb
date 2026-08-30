src = <<~SRC
  module Schemes
    class << self
      CHARS = ".+-"
      MAX = CHARS.size
      class Box; end
      def list = constants
      def chars = CHARS
      def box = Box
      private def hidden = :hidden
    end
  end
SRC
eval src, nil, "s.rb"

# The constants and the nested class belong to the SINGLETON, not the module.
p Schemes.constants
p Schemes.list
p Schemes.singleton_class.constants(false).sort
p Schemes.chars
p [Schemes.box.is_a?(Class), Schemes.box.equal?(Schemes.singleton_class.const_get(:Box))]
p((Schemes::CHARS rescue $!.class))

# The defs are class methods of the module, and `private` still holds.
p Schemes.respond_to?(:chars)
p Schemes.respond_to?(:hidden)
p Schemes.singleton_methods.sort

# `class << self` in a snippet whose self is main opens MAIN's singleton.
eval "class << self; def only_here = 7; end"
p self.singleton_class.instance_methods(false).include?(:only_here)
p only_here
