# A `class`/`module` written in a run-time `eval`. The header mints or reuses
# the class through the runtime; the BODY runs as one more `class_eval` of its
# own source, which is what a class body IS -- a scope with its own cref, its
# own locals, sharing nothing with the snippet around it.

r = eval("class Made\n  SECRET = 41\n  def hi = 'hi'\n  def sec = SECRET + 1\nend".dup)
p r
p Made.new.hi
p Made.new.sec
p Made::SECRET
p [Made.name, Made.superclass]

# A superclass expression reads in the SNIPPET's scope, and `super` walks.
eval("class Sub2 < Made\n  def hi = 'sub-' + super\nend".dup)
p Sub2.new.hi

# A module, and a scoped name.
eval("module Mod1\n  def self.who = 'mod'\nend".dup)
p [Mod1.who, Mod1.name]
module Outer; end
eval("class Outer::Inner\n  def z = 1\nend".dup)
p [Outer::Inner.new.z, Outer::Inner.name]

# A reopen finds the class the constant already names.
eval("class Made\n  def more = 2\nend".dup)
p [Made.new.more, Made.new.hi]

# The body is worth its last statement, and an empty one is nil.
p eval("class Val; 7; end".dup)
p eval("class Empty; end".dup)

# Nesting, class variables, mixins, accessors, and an enclosing cref.
TOP = 1
module Wrap
  IN_WRAP = 2
end
eval(<<~SRC.dup)
  class Deep
    class Nested
      def n = 'n'
    end
    @@cv = 5
    def self.cv = @@cv
    attr_accessor :a
    include Comparable
    def <=>(other) = 0
    def top = TOP
    ME = self
  end
SRC
p Deep::Nested.new.n
p Deep.cv
d = Deep.new
d.a = 9
p d.a
p Deep.ancestors.include?(Comparable)
p [Deep.new.top, Deep::ME, Deep::Nested.name]

# A class opened inside a `class_eval` inherits ITS cref, so a constant of the
# enclosing module is still in reach.
Wrap.class_eval("class Inside\n  def w = IN_WRAP\nend".dup)
p [Wrap::Inside.new.w, Wrap::Inside.name]
__END__
:sec
"hi"
42
41
["Made", Object]
"sub-hi"
["mod", "Mod1"]
[1, "Outer::Inner"]
[2, "hi"]
7
nil
"n"
5
9
true
[1, Deep, "Deep::Nested"]
[2, "Wrap::Inside"]
