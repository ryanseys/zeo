# A conditional `def` inside `class << self` -- erb/compiler.rb's
# `class << self; if defined?(Ractor); def register_scanner ...; else; ...`.
# zeo's static `class << self` handler rejects the `if`, so it does not yet
# match ruby here.
class Scanner
  class << self
    if RUBY_VERSION >= "3.0"
      def which
        "new"
      end
    else
      def which
        "old"
      end
    end
  end
end
puts Scanner.which
