# A top-level `def self.name` is a SINGLETON method on `main` --
# callable via implicit self at the top level, but (CRuby's asymmetry with
# a plain top-level `def`, a private Object instance method) NOT from
# inside another object's method, where self isn't `main`.

def self.only_main; "main-only"; end
def self.wrap; "[#{yield}]"; end
puts only_main
puts wrap { "b" }
class Widget
  def try; only_main rescue "not-visible"; end
end
puts Widget.new.try
__END__
main-only
[b]
not-visible
