# `Invalid yield`: which snippet shapes carry a block channel and which do not.
# A `def` opens one, so a `yield` under it compiles however deeply it is
# nested. Nothing else does -- a lambda, an ordinary block, and a
# `define_method` block are all refused, and refused at COMPILE time, before
# the snippet runs a statement.
#
# zeo got two of these wrong, in opposite directions.
#
# A `class << self` body lowers to BOTH a `DefMethod` and the
# `define_method`-shaped `Lambda` that installs it at run time, sharing one
# body. The walk arrived through the Lambda, where a guard on the `DefMethod`
# NODE never fires -- so the def's own `yield` was refused. That is bundler's
# shape: `time.rb`'s `class << self; def strptime(...) ... yield(year)` made
# `require "bundler/setup"` refuse the whole file.
#
# The other direction: a literal `define_method(:x) { ... }` desugars into a
# `DefMethod` too, and guarding on the node alone let its block's `yield`
# through, where ruby refuses. `is_def` is what tells the two apart.

SRCS = [
  "def f1; yield; end",
  "def g1; if true; yield; end; end",
  "module M1; class << self; def t(n); yield n; end; end; end",
  "module M2; def self.t(n); yield n; end; end",
  "class K1; def t; [1].each { yield }; end; end",
  "-> { yield }",
  "[1].each { yield }",
  "yield",
  "class C1; define_method(:x) { yield }; end",
  "class C2; class << self; define_method(:x) { yield }; end; end",
]

i = 0
while i < SRCS.size
  src = SRCS[i]
  begin
    eval(src)
    puts "compiles  #{src}"
  rescue SyntaxError
    puts "refused   #{src}"
  rescue StandardError => e
    puts "#{e.class}  #{src}"
  end
  i += 1
end

# The refusal is a catchable SyntaxError naming the yield's own line.
begin
  eval("1\n2\nyield\n")
rescue SyntaxError => e
  puts e.message.split(": ").last
end

# ...and a `def` written in the snippet really does yield when called.
eval("def zz(n); yield n; end")
p zz(3) { |v| v + 1 }
__END__
compiles  def f1; yield; end
compiles  def g1; if true; yield; end; end
compiles  module M1; class << self; def t(n); yield n; end; end; end
compiles  module M2; def self.t(n); yield n; end; end
compiles  class K1; def t; [1].each { yield }; end; end
refused   -> { yield }
refused   [1].each { yield }
refused   yield
refused   class C1; define_method(:x) { yield }; end
refused   class C2; class << self; define_method(:x) { yield }; end; end
Invalid yield
4
