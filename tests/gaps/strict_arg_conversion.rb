# GAP -- imported from the spinel corpus at fa06b601.
#
# Several argument-conversion TypeErrors carry zeo's message where CRuby has
# its own, and the two describe different things:
#
#   CRuby  "no implicit conversion from nil to integer"
#   zeo    "NilClass can't be coerced into Integer"
#
# CRuby's `rb_to_int`-family message names the CONVERSION that was not
# available; zeo reports a coercion failure instead. The exception CLASS
# agrees in every case, so a rescue still fires -- only a message match does
# not.
#
# The original header follows. It describes the PREDECESSOR project's
# version of this test and its own fix, not zeo's divergence above.
#
# CRuby's argument validation at builtin boundaries: a compile-time-known
# nil / true / false (or wrong-classed scalar) entering a String- or
# Integer-typed slot raises CRuby's TypeError, wording included, where the
# slot's zero ("" / 0) used to stand in silently. The slots CRuby itself
# accepts nil in keep working.

# nil / bool into Integer slots ("from nil to integer" wording)
begin
  p([10, 20, 30].take(nil))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p([10, 20, 30].rotate(nil))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p([10, 20, 30].delete_at(nil))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p([10, 20, 30].each_cons(nil))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p((1..9).first(nil))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p("ab" * nil)
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p("ab".center(nil))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p(3.4.round(nil))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p(5.to_s(nil))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p(Integer("5", nil))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p(:abc[nil])
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p([1, 2].fill(0, false))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end

# rb_to_int slots ("of nil into Integer" wording)
begin
  p(5.digits(nil))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p(5[nil])
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p(Random.srand(nil))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end

# nil / bool / wrong class into String slots
begin
  p(File.join("a", nil))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p("hello".include?(true))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p("ab".delete_prefix(nil))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p(Regexp.new(nil))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p([1].pack(1))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p(File.path(nil))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end

# the regexp-expected pattern family
begin
  p("ab".match(nil))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p("ab".sub(nil, "x"))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p("ab".scan(nil))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p("ab".match?(false))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p("ab".gsub(true, "x"))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end

# the Array-operand family
begin
  p([1, 2].concat(nil))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p([1, 2].product(nil))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end

# MatchData indexes
m = /a(b)/.match("ab")
begin
  p(m[nil])
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p(m[false])
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end

# native bindings and module functions
require "json"
require "base64"
require "strscan"
begin
  p(JSON.parse(nil))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p(Base64.encode64(nil))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p(StringScanner.new("ab").scan(nil))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p(Random.new(2).bytes(nil))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end

# slots where CRuby accepts nil stay accepting
p "a b".split(nil)
p "ab\n".chomp(nil)
p [1, [2]].flatten(nil)
p [1, 2].fill(0, nil)
p(/a/.match(nil))
p(/a/ =~ nil)
p("ab" =~ nil)
ENV["SPINEL_C3_T"] = "v"
ENV["SPINEL_C3_T"] = nil
p ENV["SPINEL_C3_T"].nil?

# review-found edges: named-capture =~ nil subject; fill's nil length is "to
# the end"; a boolean =~ operand is NoMethodError; File.read's nil length
def parse_kv(line)
  if /(?<k>\w+)=(?<v>\w+)/ =~ line
    [k, v]
  end
end
p parse_kv("a=1")
p parse_kv(nil)
p [1, 2, 3].fill(9, 1, nil)
begin
  p("ab" =~ true)
rescue NoMethodError => e
  p [e.class, e.message]
end
File.write("sac-frd.txt", "hello")
begin
  p File.read("sac-frd.txt", nil)
ensure
  File.delete("sac-frd.txt")
end

# super(nil) into a builtin exception keeps CRuby's class-name default
class ConvE < StandardError
  def initialize; super(nil); end
end
p ConvE.new.message
