# A top-level `def` reaches every object, and it must answer the same five
# questions wherever it is called from.
#
# WHY THIS FILE EXISTS. zeo emits a definition on Object/Kernel/BasicObject
# ONCE under its `Object#name` symbol, and ALSO materializes a private copy
# onto every user class -- 22,393 extra bodies on a program that requires
# rubygems, which is 78 MB of the binary. A builtin receiver already skips
# the copy and dispatches to the shared body (`analyze::mro` exempts it), so
# the two paths are live side by side RIGHT NOW.
#
# That makes this a decisive experiment rather than a guess: if a builtin
# receiver -- which uses the shared body -- answers exactly what a user class
# answers -- which uses its own copy -- then dropping the copy changes
# nothing. Every row below is printed for both kinds for that reason.
#
# The five questions are the ones a shared body could get wrong, because each
# is compiled from something the body would no longer own: an ivar slot, the
# caller's class, the constant cref, the method owner, and where `super`
# goes.

TOP_LEVEL_CONST = "top"

def touches_an_ivar
  @counter = (@counter || 0) + 1
  [@counter, instance_variable_get(:@counter), instance_variables]
end

def reads_a_constant
  TOP_LEVEL_CONST
end

def reports_its_owner
  method(__method__).owner
end

def calls_a_private_sibling
  a_private_helper
end

# Every top-level `def` is already a PRIVATE instance method of Object, which
# is why each row below reaches it through `send` rather than a dot.
def a_private_helper
  "private reached"
end

def goes_to_super
  super
rescue NoMethodError => e
  "no super: #{e.message[0, 24]}"
end

class Plain; end

class WithIvar
  def initialize = (@counter = 100)
end

class Overrides
  def goes_to_super = "overridden"
end

# A class that DEFINES the same name is not sharing anything, and must keep
# its own body.
class OwnCopy
  def reads_a_constant = "mine"
end

RECEIVERS = {
  # Builtin receivers: the shared-body path today.
  "String" => -> { +"s" },
  "Array" => -> { [1] },
  "Hash" => -> { {} },
  # User classes: the materialized-copy path today.
  "Plain" => -> { Plain.new },
  "WithIvar" => -> { WithIvar.new },
  "OwnCopy" => -> { OwnCopy.new },
}.freeze

def show(label, receiver)
  yield receiver
rescue StandardError => e
  "#{e.class}: #{e.message[0, 40]}"
end

RECEIVERS.each do |name, make|
  r = make.call
  puts "#{name}\tivar\t#{show(name, r) { |o| o.send(:touches_an_ivar) }.inspect}"
  puts "#{name}\tivar again\t#{show(name, r) { |o| o.send(:touches_an_ivar) }.inspect}"
  puts "#{name}\tconst\t#{show(name, r) { |o| o.send(:reads_a_constant) }.inspect}"
  puts "#{name}\towner\t#{show(name, r) { |o| o.send(:reports_its_owner) }.inspect}"
  puts "#{name}\tprivate\t#{show(name, r) { |o| o.send(:calls_a_private_sibling) }.inspect}"
  puts "#{name}\tsuper\t#{show(name, r) { |o| o.send(:goes_to_super) }.inspect}"
  puts "#{name}\tprivate?\t#{r.respond_to?(:a_private_helper).inspect}"
end

# The two paths must also agree about REFLECTION, which is what a gem reads.
puts "owner via Plain\t#{Plain.instance_method(:reads_a_constant).owner}"
puts "owner via String\t#{String.instance_method(:reads_a_constant).owner}"
puts "owner via OwnCopy\t#{OwnCopy.instance_method(:reads_a_constant).owner}"
puts "Plain defines?\t#{Plain.instance_methods(false).include?(:reads_a_constant)}"
puts "Object defines?\t#{Object.private_instance_methods(false).include?(:reads_a_constant)}"

# An ivar written through a shared body has to land on the RECEIVER, not
# anywhere shared. Two objects of one class must not see each other's.
a = Plain.new
b = Plain.new
a.send(:touches_an_ivar)
a.send(:touches_an_ivar)
b.send(:touches_an_ivar)
puts "separate ivars\t#{[a.instance_variable_get(:@counter), b.instance_variable_get(:@counter)].inspect}"

# And a class that already had the ivar keeps its own value.
w = WithIvar.new
w.send(:touches_an_ivar)
puts "pre-set ivar\t#{w.instance_variable_get(:@counter)}"

# `super` from a universal def, where the receiver's class DOES define it.
puts "override wins\t#{Overrides.new.goes_to_super.inspect}"
__END__
String	ivar	[1, 1, [:@counter]]
String	ivar again	[2, 2, [:@counter]]
String	const	"top"
String	owner	Object
String	private	"private reached"
String	super	"no super: super: no superclass met"
String	private?	false
Array	ivar	[1, 1, [:@counter]]
Array	ivar again	[2, 2, [:@counter]]
Array	const	"top"
Array	owner	Object
Array	private	"private reached"
Array	super	"no super: super: no superclass met"
Array	private?	false
Hash	ivar	[1, 1, [:@counter]]
Hash	ivar again	[2, 2, [:@counter]]
Hash	const	"top"
Hash	owner	Object
Hash	private	"private reached"
Hash	super	"no super: super: no superclass met"
Hash	private?	false
Plain	ivar	[1, 1, [:@counter]]
Plain	ivar again	[2, 2, [:@counter]]
Plain	const	"top"
Plain	owner	Object
Plain	private	"private reached"
Plain	super	"no super: super: no superclass met"
Plain	private?	false
WithIvar	ivar	[101, 101, [:@counter]]
WithIvar	ivar again	[102, 102, [:@counter]]
WithIvar	const	"top"
WithIvar	owner	Object
WithIvar	private	"private reached"
WithIvar	super	"no super: super: no superclass met"
WithIvar	private?	false
OwnCopy	ivar	[1, 1, [:@counter]]
OwnCopy	ivar again	[2, 2, [:@counter]]
OwnCopy	const	"mine"
OwnCopy	owner	Object
OwnCopy	private	"private reached"
OwnCopy	super	"no super: super: no superclass met"
OwnCopy	private?	false
owner via Plain	Object
owner via String	Object
owner via OwnCopy	OwnCopy
Plain defines?	false
Object defines?	true
separate ivars	[2, 1]
pre-set ivar	101
override wins	"overridden"
