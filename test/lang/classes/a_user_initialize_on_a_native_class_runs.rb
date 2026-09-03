# A native class's `new` FUSES allocate and initialize, so it cannot run a
# body it does not know about. Ruby's `new` is allocate plus whatever
# `initialize` resolves to, so once a program reopens the class with its own,
# the native construction is GONE -- `String.new("x")` answers `""` and
# `Hash.new(5)` has no default. Only `new` is affected: `Time.now` and
# `Time.at` build their own instances and never ask.

def t(label)
  r = begin
    yield.inspect
  rescue Exception => e
    "#{e.class}: #{e.message}"
  end
  puts format("%-26s %s", label, r)
end

class String
  def initialize(*) = @s = 1
end
t("String.new content") { String.new("x") }
t("String.new ivars") { String.new("x").instance_variables }
t("a literal is unaffected") { "y" }

class Time
  def initialize(*) = @t = "T"
  def mine = @t
end
t("Time.new") { [Time.new.mine, Time.new.instance_variables] }
t("Time.now unaffected") { Time.now.instance_variables }
t("Time.at unaffected") { Time.at(0).instance_variables }
t("through a variable") { k = Time; [k.new.mine, k.new.instance_variables] }
t("Class#new directly") { Class.instance_method(:new).bind(Time).call.mine }

class Hash
  def initialize(*) = @h = 1
end
t("Hash.new content") { h = Hash.new(5); [h.default, h.instance_variables] }

class Array
  def initialize(*) = @a = 1
end
t("Array.new content") { Array.new(3, 0) }
__END__
String.new content         ""
String.new ivars           [:@s]
a literal is unaffected    "y"
Time.new                   ["T", [:@t]]
Time.now unaffected        []
Time.at unaffected         []
through a variable         ["T", [:@t]]
Class#new directly         "T"
Hash.new content           [nil, [:@h]]
Array.new content          []
