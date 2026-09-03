# `class <expr>::Name` -- a definition whose NAMESPACE is a runtime value
# rather than a constant path. Four spellings in the corpus, all the same
# thing said differently, and zeo used to reject every one of them with
# "expected a constant name or path":
#
#   class self::Task            (inside an `included do ... end` hook)
#   class parent::Pagination    (a local, jekyll-localization)
#   module Wires.current_network::Namespace
#   module Num[16]::Trigonometry            (flt)
#
# JRuby's lowercase java packages are the same shape once more:
# `class org::jrubyparser::ast::CallNode` parses `org::jrubyparser` as a CALL,
# not a constant path, so lowering the namespace as an expression gives the
# `NameError` on `org` that CRuby raises when the definition runs.
module Host; end

def current = Host
parent = Host
INDEXED = [Host]

class parent::Pag
  def self.hi = "from a local"
end

module current::Ns
  def self.v = "from a method"
end

class INDEXED[0]::Deep < String
  def self.hi = "from an index"
end

Host.instance_eval do
  module self::Hooked
    def self.hi = "from self"
  end
end

module Host.itself::Nested
  module Inner
    def self.go = "nested inner"
  end

  def self.direct = "nested direct"
end

p Host::Pag.hi
p Host::Pag.name
p Host::Ns.v
p Host::Ns.name
p Host::Deep.hi
p Host::Deep.superclass
p Host::Hooked.hi
p Host::Nested::Inner.go
p Host::Nested.direct
p Host.constants.map(&:to_s).sort

# The namespace expression is evaluated where the definition is written, so a
# raise there is the definition's raise.
begin
  class org::foo::Bar
  end
rescue NameError => e
  puts e.message
end
p defined?(Host::Missing).nil?
__END__
"from a local"
"Host::Pag"
"from a method"
"Host::Ns"
"from an index"
String
"from self"
"nested inner"
"nested direct"
["Deep", "Hooked", "Nested", "Ns", "Pag"]
undefined local variable or method 'org' for main
true
