# `"lit".freeze` on a string LITERAL answers the interned fstring, so two of
# them are the SAME object -- and CRuby REFUSES a singleton on one.
#
# The two halves ship together on purpose. Interning without the refusal would
# let a program attach a singleton to a string another part of the program is
# also holding, which is worse than the divergence it fixes.
#
# The rule is a COMPILE-TIME one about the literal RECEIVER, not about
# `String#freeze`: CRuby compiles `<string literal>.freeze` to
# `opt_str_freeze`, which answers the fstring table's entry. Freezing a string
# that arrived any other way (`s = +"x"; s.freeze`) allocates nothing and
# dedups nothing, on both engines.
#
# zeo already had the pool -- `String#-@` interns and `-"lit"` was correct --
# so the fold routes a literal `.freeze` into the same entry the
# `# frozen_string_literal: true` path already uses. It is guarded on `freeze`
# not being reopened, exactly as CRuby guards `opt_str_freeze` on its own
# redefinition flag.
#
# The refusal asks by object IDENTITY, not by a flag: the pool holds one
# string per (bytes, encoding), so being interned means being THAT object.
# All three singleton-CREATING surfaces refuse; the reading ones do not.
p "lit".freeze.equal?("lit".freeze)
p (-"lit").equal?(-"lit")
p "lit".freeze.frozen?

s = +"built"
s.freeze
p s.equal?("built".freeze)

begin
  "lit".freeze.singleton_class
rescue => e
  p [e.class, e.message]
end

t = +"unshared"
t.freeze
p t.singleton_class.equal?(t.singleton_class)

# The sweep.

# All three singleton-creating surfaces refuse; reading ones answer.
f = "shared".freeze
[[:singleton_class, -> { f.singleton_class }],
 [:define_singleton_method, -> { f.define_singleton_method(:x) { 1 } }],
 [:extend, -> { f.extend(Module.new) }],
 [:singleton_methods, -> { f.singleton_methods }],
 [:dup_then_singleton, -> { f.dup.singleton_class }]].each do |name, probe|
  begin
    p [name, probe.call.class]
  rescue => e
    p [name, e.class, e.message]
  end
end

# An interpolated literal is a fresh string every time: nothing to intern.
x = 1
p ["a#{x}b".freeze.equal?("a#{x}b".freeze), "a#{x}b".freeze.frozen?]

# The fold answers the same object `-@` does -- one pool, not two.
p "pool".freeze.equal?(-"pool")
p "pool".freeze.equal?("pool".dup.freeze)

# Mutating an fstring raises, and names it.
begin
  "immutable".freeze << "x"
rescue => e
  p [e.class, e.message]
end

# An empty literal and a literal holding a NUL byte both intern by content.
p ["".freeze.equal?("".freeze), "a\0b".freeze.equal?("a\0b".freeze)]

# Equal content in DIFFERENT encodings is not the same object.
p "enc".freeze.equal?("enc".dup.force_encoding("ASCII-8BIT").freeze)

# A frozen literal is still `==` to an unfrozen twin.
p ["eq".freeze == +"eq", "eq".freeze.eql?(+"eq")]

__END__
true
true
true
false
[TypeError, "can't define singleton"]
true
[:singleton_class, TypeError, "can't define singleton"]
[:define_singleton_method, TypeError, "can't define singleton"]
[:extend, TypeError, "can't define singleton"]
[:singleton_methods, Array]
[:dup_then_singleton, Class]
[false, true]
true
false
[FrozenError, "can't modify frozen String: \"immutable\""]
[true, true]
false
[true, true]
