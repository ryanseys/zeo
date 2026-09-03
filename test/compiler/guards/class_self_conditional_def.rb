# A conditional `def` inside `class << self` -- erb/compiler.rb's
# `class << self; if defined?(Ractor); def register_scanner ...; else; ...`.
# zeo statically decides the version/`defined?` guard and registers ONLY the
# taken branch's class methods, matching ruby (which runs one branch).
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
__END__
new
