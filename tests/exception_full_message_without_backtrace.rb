# `full_message` renders through CRuby's `rb_error_write0`: the head line, the
# `from` trail, then the cause chain. An exception carrying NO backtrace hangs
# the head off the CALLER's own position under the reporting method's name,
# which is why the first line below names `full_message`.
e = StandardError.new("msg")
p e.backtrace
p e.full_message(highlight: false, order: :top)

# An empty message prints the class alone -- `unhandled exception` for a bare
# RuntimeError, which is what an argumentless `raise` builds.
p RuntimeError.new("").full_message(highlight: false)
p StandardError.new("").full_message(highlight: false)
p StandardError.new("").detailed_message(highlight: false)
p RuntimeError.new("").detailed_message(highlight: true)

# `order:` picks the direction. `:bottom` numbers the trail, right-aligned to
# the widest index, under the Traceback banner.
b = StandardError.new("msg")
b.set_backtrace(["a.rb:1:in 'x'", "b.rb:2:in 'y'", "c.rb:3:in 'z'"])
p b.full_message(highlight: false, order: :top)
p b.full_message(highlight: false, order: :bottom)
p b.full_message(highlight: true, order: :top)
p b.full_message(highlight: true, order: :bottom)
wide = StandardError.new("m")
wide.set_backtrace((1..12).map { |i| "f#{i}.rb:#{i}:in 'g'" })
puts wide.full_message(highlight: false, order: :bottom)

# Both keywords refuse anything else, in CRuby's own words.
begin; b.full_message(order: :sideways); rescue => x; p [x.class, x.message]; end
begin; b.full_message(highlight: 3); rescue => x; p [x.class, x.message]; end

# `detailed_message` is SENT, not computed -- an override changes every report.
class Custom < StandardError
  def detailed_message(**opt) = "CUSTOM(#{opt[:highlight]})"
end
c = Custom.new("m")
c.set_backtrace(["a.rb:1:in 'x'"])
p c.full_message(highlight: false)
p c.full_message(highlight: true)

# A multi-line message keeps the class tag on its first line.
class Wide < StandardError
  def to_s = "line one\nline two"
end
w = Wide.new
w.set_backtrace(["a.rb:1:in 'x'"])
p w.full_message(highlight: false)
p w.full_message(highlight: true)

# The cause chain renders after the exception it caused (before it, reversed).
begin
  begin
    raise "inner"
  rescue
    raise "outer"
  end
rescue => chained
  p chained.full_message(highlight: false, order: :top)
  p chained.full_message(highlight: false, order: :bottom)
end
