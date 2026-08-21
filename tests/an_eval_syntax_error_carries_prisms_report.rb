# A snippet that does not parse raises a `SyntaxError` whose message is the
# program's to read, so it is prism's whole report -- header, the offending
# line with its context, and one caret row per diagnostic -- exactly as
# CRuby builds it.

def report(src, file = nil, line = nil)
  eval(src, nil, *[file, line].compact)
rescue SyntaxError => e
  puts "----- #{src.inspect}"
  puts e.message
end

report("1 +", "F.rb", 7)
report("foo(", "F.rb", 7)
report("  x ]", "F.rb", 7)
report("a = 1\na = 1\nb +", "F.rb", 4)
report("\n" * 120 + "1 +", "F.rb", 3)
report("x = 'ありがとう' +\n", "F.rb", 3)

# With no file argument the rows name where the eval was written.
report("1 +")

# A snippet that does not parse is a SyntaxError, never a compiler refusal.
begin
  eval([":ok", "1 +"].last)
rescue SyntaxError => e
  puts e.class
end
