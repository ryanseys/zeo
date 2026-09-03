# A top-level `def self.name` / `class << self` installs SINGLETON methods on
# the `main` object (Batch G) -- callable via implicit self at the top level,
# but not from inside another object (CRuby's asymmetry with a plain top-level
# `def`, which is a private Object instance method).

def self.only_main
  "main-only"
end

def self.wrap
  "[#{yield}]"
end

class << self
  def shout
    "MAIN"
  end
end

puts only_main
puts wrap { "inner" }
puts shout

class Widget
  def try
    only_main rescue "not visible in Widget"
  end
end
puts Widget.new.try
__END__
main-only
[inner]
MAIN
not visible in Widget
