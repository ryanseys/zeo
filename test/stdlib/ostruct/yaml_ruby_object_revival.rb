# `!ruby/...` tags, and the three ways an object gets filled back in.
#
# This is the half of psych that turns a document into OBJECTS rather than
# into hashes: `Gem::Specification.from_yaml` reads a `.gem` through it, and
# before it existed a `!ruby/object:Gem::Specification` loaded as a Hash and
# rubygems reported "YAML data doesn't evaluate to gem specification".
#
# Psych fills a revived object in a fixed order, and each rung is here:
# `init_with(coder)` first, `yaml_initialize(tag, map)` next, and plain ivar
# assignment last.

require "yaml"
require "ostruct"

# --- the three fill routes ------------------------------------------------
class PlainIvars
  attr_reader :name, :size
  def inspect = "#<PlainIvars name=#{@name.inspect} size=#{@size.inspect}>"
end

class KnowsInitWith
  attr_reader :seen, :tag
  def init_with(coder)
    @tag = coder.tag
    @seen = coder.map
  end
  def inspect = "#<KnowsInitWith tag=#{@tag.inspect} seen=#{@seen.inspect}>"
end

class KnowsYamlInitialize
  attr_reader :seen, :tag
  def yaml_initialize(tag, vals)
    @tag = tag
    @seen = vals
  end
  def inspect = "#<KnowsYamlInitialize tag=#{@tag.inspect} seen=#{@seen.inspect}>"
end

# A class whose `initialize` refuses no-argument construction: psych must
# ALLOCATE rather than call `new`, and this is what proves it does.
class RefusesNew
  attr_reader :a
  def initialize(a) = (@a = a)
  def inspect = "#<RefusesNew a=#{@a.inspect}>"
end

Point = Struct.new(:x, :y)

def show(label)
  puts "#{label}\t#{yield.inspect}"
rescue StandardError => e
  puts "#{label}\t#{e.class}: #{e.message}"
end

show("plain ivars") do
  YAML.unsafe_load("--- !ruby/object:PlainIvars\nname: bolt\nsize: 3\n")
end

show("init_with") do
  YAML.unsafe_load("--- !ruby/object:KnowsInitWith\na: 1\nb: two\n")
end

show("yaml_initialize") do
  YAML.unsafe_load("--- !ruby/object:KnowsYamlInitialize\na: 1\n")
end

show("allocate not new") do
  YAML.unsafe_load("--- !ruby/object:RefusesNew\na: 9\n")
end

# --- the other reviving tags ----------------------------------------------
show("struct") { YAML.unsafe_load("--- !ruby/struct:Point\nx: 1\ny: 2\n") }
show("struct member order") do
  # The document writes the members backwards; the struct still fills by NAME.
  YAML.unsafe_load("--- !ruby/struct:Point\ny: 2\nx: 1\n").to_a
end
show("exception") { YAML.unsafe_load("--- !ruby/exception:ArgumentError\nmessage: bad\n") }
show("exception message") do
  YAML.unsafe_load("--- !ruby/exception:ArgumentError\nmessage: bad\n").message
end
show("class") { YAML.unsafe_load("--- !ruby/class 'String'\n") }
show("module") { YAML.unsafe_load("--- !ruby/module 'Comparable'\n") }
show("regexp") { YAML.unsafe_load("--- !ruby/regexp /ab+c/im\n") }
show("regexp matches") { YAML.unsafe_load("--- !ruby/regexp /ab+c/im\n") =~ "xABBC" }
show("regexp with slash") { YAML.unsafe_load("--- !ruby/regexp /a\\/b/\n").source }
show("string subclass") { YAML.unsafe_load("--- !ruby/string:String hello\n") }
show("array subclass") { YAML.unsafe_load("--- !ruby/array:Array\n- 1\n- 2\n") }
show("hash subclass") { YAML.unsafe_load("--- !ruby/hash:Hash\na: 1\n") }
show("complex") { YAML.unsafe_load("--- !ruby/object:Complex\nreal: 1\nimage: 2\n") }
show("rational") { YAML.unsafe_load("--- !ruby/object:Rational\nnumerator: 1\ndenominator: 2\n") }
show("range") { YAML.unsafe_load("--- !ruby/range\nbegin: 1\nend: 5\nexcl: false\n") }
show("ostruct") { YAML.unsafe_load("--- !ruby/object:OpenStruct\ntable:\n  :a: 1\n") }
show("set") { YAML.unsafe_load("--- !!set\n? a\n? b\n") }
show("set class") { YAML.unsafe_load("--- !!set\n? a\n? b\n").class }
show("omap map") { YAML.unsafe_load("--- !!omap\na: 1\nb: 2\n").class }
show("omap seq") { YAML.unsafe_load("--- !!omap\n- a: 1\n- b: 2\n") }

# A name nothing defines is an error, not a bare Object carrying the ivars.
show("unknown class") { YAML.unsafe_load("--- !ruby/object:NoSuchClassHere\nx: 1\n") }

# --- what safe_load refuses, and what naming a class permits --------------
GATED = {
  "object" => "--- !ruby/object:PlainIvars\nname: bolt\n",
  "struct" => "--- !ruby/struct:Point\nx: 1\ny: 2\n",
  "exception" => "--- !ruby/exception:ArgumentError\nmessage: bad\n",
  "class" => "--- !ruby/class 'String'\n",
  "module" => "--- !ruby/module 'Comparable'\n",
  "regexp" => "--- !ruby/regexp /a/\n",
  "string sub" => "--- !ruby/string:String hi\n",
  "array sub" => "--- !ruby/array:Array\n- 1\n",
  "hash sub" => "--- !ruby/hash:Hash\na: 1\n",
  "range" => "--- !ruby/range\nbegin: 1\nend: 2\nexcl: false\n",
  "set" => "--- !!set\n? a\n",
  "omap map" => "--- !!omap\na: 1\n",
  # A sequence is the one shape psych does NOT gate: its sequence visitor
  # never reaches the class loader for these tags.
  "omap seq" => "--- !!omap\n- a: 1\n",
  "object seq" => "--- !ruby/object:PlainIvars\n- 1\n",
  "range seq" => "--- !ruby/range\n- 1\n",
  "set seq" => "--- !!set\n- a\n",
}.freeze

GATED.each do |name, doc|
  show("gated #{name}") { YAML.safe_load(doc) }
end

# Naming the class is what lets it through.
show("permitted object") do
  YAML.safe_load("--- !ruby/object:PlainIvars\nname: bolt\n", permitted_classes: ["PlainIvars"])
end
show("permitted struct") do
  YAML.safe_load("--- !ruby/struct:Point\nx: 1\ny: 2\n", permitted_classes: ["Point"])
end
show("permitted set") do
  YAML.safe_load("--- !!set\n? a\n", permitted_classes: ["Psych::Set"]).class
end

# --- a revived object survives a round trip through the tree --------------
show("parse then to_ruby") do
  YAML.parse("--- !ruby/object:PlainIvars\nname: bolt\nsize: 3\n").to_ruby
end
__END__
plain ivars	#<PlainIvars name="bolt" size=3>
init_with	#<KnowsInitWith tag="!ruby/object:KnowsInitWith" seen={"a" => 1, "b" => "two"}>
yaml_initialize	#<KnowsYamlInitialize tag=nil seen=nil>
allocate not new	#<RefusesNew a=9>
struct	#<struct Point x=1, y=2>
struct member order	[1, 2]
exception	#<ArgumentError: bad>
exception message	"bad"
class	String
module	Comparable
regexp	/ab+c/mi
regexp matches	1
regexp with slash	"a/b"
string subclass	"hello"
array subclass	[1, 2]
hash subclass	{"a" => 1}
complex	(1+2i)
rational	(1/2)
range	1..5
ostruct	#<OpenStruct a=1>
set	{"a" => nil, "b" => nil}
set class	Psych::Set
omap map	Psych::Omap
omap seq	{"a" => 1, "b" => 2}
unknown class	ArgumentError: undefined class/module NoSuchClassHere
gated object	Psych::DisallowedClass: Tried to load unspecified class: PlainIvars
gated struct	Psych::DisallowedClass: Tried to load unspecified class: Point
gated exception	Psych::DisallowedClass: Tried to load unspecified class: ArgumentError
gated class	Psych::DisallowedClass: Tried to load unspecified class: String
gated module	Psych::DisallowedClass: Tried to load unspecified class: Comparable
gated regexp	Psych::DisallowedClass: Tried to load unspecified class: Regexp
gated string sub	Psych::DisallowedClass: Tried to load unspecified class: String
gated array sub	Psych::DisallowedClass: Tried to load unspecified class: Array
gated hash sub	Psych::DisallowedClass: Tried to load unspecified class: Hash
gated range	Psych::DisallowedClass: Tried to load unspecified class: Range
gated set	Psych::DisallowedClass: Tried to load unspecified class: Psych::Set
gated omap map	Psych::DisallowedClass: Tried to load unspecified class: Psych::Omap
gated omap seq	{"a" => 1}
gated object seq	[1]
gated range seq	[1]
gated set seq	["a"]
permitted object	#<PlainIvars name="bolt" size=nil>
permitted struct	Psych::DisallowedClass: Tried to load unspecified class: Symbol
permitted set	Psych::Set
parse then to_ruby	#<PlainIvars name="bolt" size=3>
