# The dominant `class << self` stdlib idiom (e.g.
# URI's `class << self; RESERVED = ...; def escape; ...RESERVED...; end`)
# defines constants alongside the class methods that reference them. The
# constant is spliced onto the enclosing class, whose class methods resolve
# it lexically (oracle-verified).

class Config
  class << self
    PREFIX = "cfg:"
    LIMIT = 3
    def key(n); "#{PREFIX}#{n}"; end
    def capped(n); n > LIMIT ? LIMIT : n; end
  end
end
puts Config.key("host")
puts Config.capped(9)
__END__
cfg:host
3
