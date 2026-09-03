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
__END__
nil
"lang/exceptions/exception_full_message_without_backtrace.rb:7:in 'full_message': msg (StandardError)\n"
"lang/exceptions/exception_full_message_without_backtrace.rb:11:in 'full_message': unhandled exception\n"
"lang/exceptions/exception_full_message_without_backtrace.rb:12:in 'full_message': StandardError\n"
"StandardError"
"\e[1;4munhandled exception\e[m"
"a.rb:1:in 'x': msg (StandardError)\n\tfrom b.rb:2:in 'y'\n\tfrom c.rb:3:in 'z'\n"
"Traceback (most recent call last):\n\t2: from c.rb:3:in 'z'\n\t1: from b.rb:2:in 'y'\na.rb:1:in 'x': msg (StandardError)\n"
"a.rb:1:in 'x': \e[1mmsg (\e[1;4mStandardError\e[m\e[1m)\e[m\n\tfrom b.rb:2:in 'y'\n\tfrom c.rb:3:in 'z'\n"
"\e[1mTraceback\e[m (most recent call last):\n\t2: from c.rb:3:in 'z'\n\t1: from b.rb:2:in 'y'\na.rb:1:in 'x': \e[1mmsg (\e[1;4mStandardError\e[m\e[1m)\e[m\n"
Traceback (most recent call last):
	11: from f12.rb:12:in 'g'
	10: from f11.rb:11:in 'g'
	 9: from f10.rb:10:in 'g'
	 8: from f9.rb:9:in 'g'
	 7: from f8.rb:8:in 'g'
	 6: from f7.rb:7:in 'g'
	 5: from f6.rb:6:in 'g'
	 4: from f5.rb:5:in 'g'
	 3: from f4.rb:4:in 'g'
	 2: from f3.rb:3:in 'g'
	 1: from f2.rb:2:in 'g'
f1.rb:1:in 'g': m (StandardError)
[ArgumentError, "expected :top or :bottom as order: :sideways"]
[ArgumentError, "expected true or false as highlight: 3"]
"a.rb:1:in 'x': CUSTOM(false)\n"
"a.rb:1:in 'x': CUSTOM(true)\n"
"a.rb:1:in 'x': line one (Wide)\nline two\n"
"a.rb:1:in 'x': \e[1mline one (\e[1;4mWide\e[m\e[1m)\e[m\n\e[1mline two\e[m\n"
"lang/exceptions/exception_full_message_without_backtrace.rb:55:in '<main>': outer (RuntimeError)\nlang/exceptions/exception_full_message_without_backtrace.rb:53:in '<main>': inner (RuntimeError)\n"
"Traceback (most recent call last):\nlang/exceptions/exception_full_message_without_backtrace.rb:53:in '<main>': inner (RuntimeError)\nlang/exceptions/exception_full_message_without_backtrace.rb:55:in '<main>': outer (RuntimeError)\n"
