# Every method of a RUNTIME-minted class answers `source_location`.
#
# `parameters` and `arity` were already right, because both are derived from
# the proc the body arrives as. The LOCATION was dropped on the floor:
# `record_runtime_params` built its row with `source: None` while holding the
# very proc that knows where it was written.
#
# So an entire family answered nil -- `Class.new { def m; end }`, a subclass
# of one, a `define_method` body, and every method of a `Data.define`
# subclass. The last is why a Data subclass's backtrace frame said
# `Object#initialize`: with no row there is nothing to name it with.
#
# `Struct.new(:a)` is compiled to a REAL class (`as_compiled_struct`), so its
# subclass never took this path and was correct all along. That asymmetry is
# what narrowed this: the two shapes are written the same way and only one
# worked.

# --- a runtime-minted class, both spellings of a definition
K = Class.new do
  def m(a); end
  define_method(:dm) { |b| }
end
p ["K#m", K.instance_method(:m).source_location]
p ["K#dm", K.instance_method(:dm).source_location]

# --- a subclass of one
class K2 < K
  def n(c); end
end
p ["K2#n", K2.instance_method(:n).source_location]

# --- a Data subclass: the shape that surfaced this
D = Data.define(:x)
class D2 < D
  def q(d); end
  def initialize(**kw); super; end
end
p ["D2#q", D2.instance_method(:q).source_location]
p ["D2#initialize", D2.instance_method(:initialize).source_location]

# --- the Struct twin, which never regressed and must not start
S = Struct.new(:a)
class S2 < S
  def r(e); end
end
p ["S2#r", S2.instance_method(:r).source_location]

# --- a class method installed at run time
K3 = Class.new do
  def self.cm(f); end
end
p ["K3.cm", K3.method(:cm).source_location]

# --- a per-object singleton
o = Object.new
def o.sing(g); end
p ["o.sing", o.method(:sing).source_location]

# --- what must STILL answer nil: a builtin has no Ruby body to point at
p ["String#upcase", String.instance_method(:upcase).source_location]
p ["Integer#+", Integer.instance_method(:+).source_location]

# --- the other two reflection channels stay right
p ["K#m params", K.instance_method(:m).parameters]
p ["K#dm params", K.instance_method(:dm).parameters]
p ["D2#q arity", D2.instance_method(:q).arity]

# --- a Method object carries it too
p ["bound", K.new.method(:m).source_location]
p ["unbound->bound", K.instance_method(:m).bind(K.new).source_location]
__END__
["K#m", ["lang/methods/a_runtime_defined_method_knows_where_it_was_written.rb", 20]]
["K#dm", ["lang/methods/a_runtime_defined_method_knows_where_it_was_written.rb", 21]]
["K2#n", ["lang/methods/a_runtime_defined_method_knows_where_it_was_written.rb", 28]]
["D2#q", ["lang/methods/a_runtime_defined_method_knows_where_it_was_written.rb", 35]]
["D2#initialize", ["lang/methods/a_runtime_defined_method_knows_where_it_was_written.rb", 36]]
["S2#r", ["lang/methods/a_runtime_defined_method_knows_where_it_was_written.rb", 44]]
["K3.cm", ["lang/methods/a_runtime_defined_method_knows_where_it_was_written.rb", 50]]
["o.sing", ["lang/methods/a_runtime_defined_method_knows_where_it_was_written.rb", 56]]
["String#upcase", nil]
["Integer#+", nil]
["K#m params", [[:req, :a]]]
["K#dm params", [[:req, :b]]]
["D2#q arity", 1]
["bound", ["lang/methods/a_runtime_defined_method_knows_where_it_was_written.rb", 20]]
["unbound->bound", ["lang/methods/a_runtime_defined_method_knows_where_it_was_written.rb", 20]]
