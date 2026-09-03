# `class << self` at the top level reopens `main`'s singleton:
# it desugars to `self.define_singleton_method(...)`, `self` being `main`,
# so each inner `def` installs on `main` and is callable via implicit self.

class << self
  def shout; "MAIN"; end
  def echo(x); "<#{x}>"; end
end
puts shout
puts echo("hi")
__END__
MAIN
<hi>
