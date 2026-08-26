# A constant written in a `class << self` body belongs to the SINGLETON
# class, not to the module. The compile-time path gets this right (see
# `tests/singleton_body_class_and_self_path.rb`); a body compiled at RUN
# TIME does not -- the constant lands on the module, so `Module#constants`
# reports a name ruby does not.
#
# `rubygems/vendor/uri/lib/uri/common.rb` is the shape: its `Schemes.list`
# maps `constants` into a Hash, and under zeo that Hash carries the
# implementation's own `ReservedChars`/`EscapedChars` rather than the
# registered schemes.
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
