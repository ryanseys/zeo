# A class MINTED at runtime from an exception root is an exception like any
# other: `.new` builds the native instance every inherited `Exception` method
# reads, `raise` accepts it in all three of its shapes, and a `rescue` clause
# matches it by ancestry.
R = Class.new(StandardError)
p R.ancestors.first(3)
p R.new("hi").message
p R.new.message
p R.superclass

# A body: an override, an addition, and a custom `initialize` that calls super.
Coded = Class.new(StandardError) do
  attr_reader :code
  def initialize(code)
    @code = code
    super("code #{code}")
  end
  def to_s = "<#{@code}>"
end
e = Coded.new(42)
p [e.code, e.message, e.to_s]

# Deeper than one level, and off a root other than StandardError.
Sub = Class.new(Coded)
p Sub.new(7).message
p Sub.ancestors.include?(StandardError)
p Class.new(ArgumentError).new("a").is_a?(ArgumentError)
p Class.new(Exception).new("x").is_a?(StandardError)

def caught
  yield
rescue => e
  [e.class.name, e.message]
end
p(caught { raise R })
p(caught { raise R, "with message" })
p(caught { raise R.new("prebuilt") })

# Matching: by an ancestor, and by a sibling that misses.
begin
  raise Sub, 3
rescue Coded => e
  p [:by_parent, e.message, e.class == Sub]
end

begin
  begin
    raise R, "inner"
  rescue Coded
    p :wrong
  end
rescue R => e
  p [:not_matched_by_sibling, e.message]
end

# An anonymous one carries the same surface.
anon = Class.new(RuntimeError).new("nameless")
p anon.class.ancestors.include?(RuntimeError)
p anon.message
p anon.backtrace

err = R.new("surface")
err.set_backtrace(["a.rb:1"])
p err.backtrace
p err.full_message(highlight: false, order: :top).lines.first.chomp
p [err.cause, err.frozen?]
__END__
[R, StandardError, Exception]
"hi"
"R"
StandardError
[42, "<42>", "<42>"]
"<7>"
true
true
false
["R", "R"]
["R", "with message"]
["R", "prebuilt"]
[:by_parent, "<3>", true]
[:not_matched_by_sibling, "inner"]
true
"nameless"
nil
["a.rb:1"]
"a.rb:1: surface (R)"
[nil, false]
