# CRuby's argument-conversion TypeErrors, message included.
#
# Four rules, each spelled where CRuby spells it: a `Check_Type` message
# (`wrong argument type X (expected Y)`) names `nil`/`true`/`false` as the
# VALUE rather than the class; `rb_num2long` says "no implicit conversion
# from nil to integer" where `rb_to_int` says "of nil into Integer";
# `StringScanner`'s pattern slot reads with `StringValue`, so its refusal
# is the STRING conversion's; and a rounding digit count, a radix and a
# random seed are conversions, never arithmetic coercions.
#
# The original header follows. It describes the PREDECESSOR project's
# version of this test and its own fix.
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
__END__
[TypeError, "no implicit conversion from nil to integer"]
[TypeError, "no implicit conversion from nil to integer"]
[TypeError, "no implicit conversion from nil to integer"]
[TypeError, "no implicit conversion from nil to integer"]
[TypeError, "no implicit conversion from nil to integer"]
[TypeError, "no implicit conversion from nil to integer"]
[TypeError, "no implicit conversion from nil to integer"]
[TypeError, "no implicit conversion from nil to integer"]
[TypeError, "no implicit conversion from nil to integer"]
[TypeError, "no implicit conversion from nil to integer"]
[TypeError, "no implicit conversion from nil to integer"]
[TypeError, "no implicit conversion of false into Integer"]
[TypeError, "no implicit conversion of nil into Integer"]
[TypeError, "no implicit conversion of nil into Integer"]
[TypeError, "no implicit conversion of nil into Integer"]
[TypeError, "no implicit conversion of nil into String"]
[TypeError, "no implicit conversion of true into String"]
[TypeError, "no implicit conversion of nil into String"]
[TypeError, "no implicit conversion of nil into String"]
[TypeError, "no implicit conversion of Integer into String"]
[TypeError, "no implicit conversion of nil into String"]
[TypeError, "wrong argument type nil (expected Regexp)"]
[TypeError, "wrong argument type nil (expected Regexp)"]
[TypeError, "wrong argument type nil (expected Regexp)"]
[TypeError, "wrong argument type false (expected Regexp)"]
[TypeError, "wrong argument type true (expected Regexp)"]
[TypeError, "no implicit conversion of nil into Array"]
[TypeError, "no implicit conversion of nil into Array"]
[TypeError, "no implicit conversion from nil to integer"]
[TypeError, "no implicit conversion of false into Integer"]
[TypeError, "no implicit conversion of nil into String"]
[TypeError, "no implicit conversion of nil into String"]
[TypeError, "no implicit conversion of nil into String"]
[TypeError, "no implicit conversion of nil into Integer"]
["a", "b"]
"ab\n"
[1, 2]
[0, 0]
nil
nil
nil
true
["a", "1"]
nil
[1, 9, 9]
[NoMethodError, "undefined method '=~' for true"]
"hello"
"ConvE"
