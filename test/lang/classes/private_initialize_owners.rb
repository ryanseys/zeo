# Two patterns behind 56 private `owner` rows in the surface comparison; this
# pins half of them.
#
# CRuby declares a private `initialize` on every class whose constructor takes
# arguments `Exception`'s does not, and reflection has to say so.
# zeo's exception tree already registered each body -- flat dispatch puts a row
# on every id -- but marked only `Exception` as the OWNER, so all eleven
# subclasses named `Exception` back.
#
# The copy hooks are the second pattern: ruby owns `initialize_copy`,
# `initialize_dup` and `initialize_clone` on KERNEL. zeo defined the first as a
# top-level `def` in the compiler prelude, which lands in Object's own_methods
# -- making it the one name in `Object.private_instance_methods(false)`, where
# ruby answers `[]`, and leaving the other two absent entirely.

def t(label)
  v = yield
  puts "#{label} => #{v.inspect}"
rescue Exception => e
  puts "#{label} !! #{e.class}: #{e.message}"
end

OWNERS = [FrozenError, Interrupt, KeyError, NameError, NoMatchingPatternKeyError,
          NoMethodError, SignalException, SyntaxError, SystemCallError, SystemExit,
          UncaughtThrowError].freeze

p OWNERS.map { |c| [c.to_s, c.private_instance_methods(false).sort] }
p OWNERS.map { |c| c.instance_method(:initialize).owner.to_s }

# A class that does NOT declare its own still points at Exception, which is
# what makes the list above a real distinction rather than a blanket mark.
p([RuntimeError, StandardError, ArgumentError].map do |c|
  [c.private_instance_methods(false), c.instance_method(:initialize).owner.to_s]
end)

# Each of those initializers takes the arguments that earned it the row.
t("FrozenError") { e = FrozenError.new("f", receiver: [1]); [e.message, e.receiver] }
t("NoMethodError") { e = NoMethodError.new("m", :nm, [1, 2]); [e.message, e.name, e.args] }
t("SyntaxError") { e = SyntaxError.new("s"); [e.message, e.path] }
t("NameError") { e = NameError.new("n", :nn); [e.message, e.name] }
t("KeyError") { e = KeyError.new("k", receiver: {}, key: :z); [e.message, e.key] }
t("SystemExit") { e = SystemExit.new(3, "bye"); [e.message, e.status] }
t("Interrupt") { e = Interrupt.new; [e.message, e.signo] }
t("SignalException") { e = SignalException.new("TERM"); [e.message, e.signm] }
t("SystemCallError") { e = SystemCallError.new("s", 2); [e.class.to_s, e.errno] }

# `Exception` keeps the two rows only IT owns.
t("Exception private") { Exception.private_instance_methods(false).sort }

# ---- the copy hooks --------------------------------------------------------
t("Object private") { Object.private_instance_methods(false).sort }
t("Kernel copy hooks") { Kernel.private_instance_methods(false).grep(/^initialize/).sort }
t("owner copy") { Object.instance_method(:initialize_copy).owner }
t("owner dup") { Object.instance_method(:initialize_dup).owner }
t("owner clone") { Object.instance_method(:initialize_clone).owner }
t("arities") do
  %i[initialize_copy initialize_dup initialize_clone].map { |m| Object.instance_method(m).arity }
end
t("private?") { Object.private_method_defined?(:initialize_copy) }
t("respond_to?") { Object.new.respond_to?(:initialize_copy, true) }

# A user override still runs, and its bare `super` still resolves -- the reason
# the hook was in the prelude at all.
class Box
  attr_accessor :v

  def initialize(v) = (@v = v)

  def initialize_copy(orig)
    super
    @v = orig.v.dup
  end
end
b = Box.new(+"hi")
c = b.dup
c.v << "!"
t("override ran") { [b.v, c.v] }
t("clone too") { d = b.clone; d.v << "?"; [b.v, d.v] }

class Plain
  attr_accessor :v
  def initialize(v) = (@v = v)
end
t("shallow dup") { p1 = Plain.new([1]); p2 = p1.dup; [p1.v.equal?(p2.v), p1.equal?(p2)] }
t("frozen clone") { o = Plain.new(1).freeze; [o.clone.frozen?, o.dup.frozen?] }
__END__
[["FrozenError", [:initialize]], ["Interrupt", [:initialize]], ["KeyError", [:initialize]], ["NameError", [:initialize]], ["NoMatchingPatternKeyError", [:initialize]], ["NoMethodError", [:initialize]], ["SignalException", [:initialize]], ["SyntaxError", [:initialize]], ["SystemCallError", [:initialize]], ["SystemExit", [:initialize]], ["UncaughtThrowError", [:initialize]]]
["FrozenError", "Interrupt", "KeyError", "NameError", "NoMatchingPatternKeyError", "NoMethodError", "SignalException", "SyntaxError", "SystemCallError", "SystemExit", "UncaughtThrowError"]
[[[], "Exception"], [[], "Exception"], [[], "Exception"]]
FrozenError => ["f", [1]]
NoMethodError => ["m", :nm, [1, 2]]
SyntaxError => ["s", nil]
NameError => ["n", :nn]
KeyError => ["k", :z]
SystemExit => ["bye", 3]
Interrupt => ["Interrupt", 2]
SignalException => ["SIGTERM", "SIGTERM"]
SystemCallError => ["Errno::ENOENT", 2]
Exception private => [:initialize, :method_missing, :respond_to_missing?]
Object private => [:t]
Kernel copy hooks => [:initialize_clone, :initialize_copy, :initialize_dup]
owner copy => Kernel
owner dup => Kernel
owner clone => Kernel
arities => [1, 1, -1]
private? => true
respond_to? => true
override ran => ["hi", "hi!"]
clone too => ["hi", "hi?"]
shallow dup => [true, false]
frozen clone => [true, false]
