# `each` answers the receiver, and a boxed receiver is no different -- but the
# call was typed only when the analyzer could prove the value carried a
# container, so a JSON.parse result (a native call returning an untyped value)
# left the chain with nothing to dispatch on and was refused at compile time.
require "json"

p JSON.parse("[]").each {}.size
p JSON.parse("[1,2]").each {}.size
p JSON.parse("[1,2]").each { |x| x }.size
p JSON.parse("{\"a\":1}").each {}.size
p JSON.parse("[1,2]").each_with_index {}.size
p JSON.parse("[[1],[2]]").each { |r| }.first
p JSON.parse("[1,2]").each {}.class

# each_entry yields what each yields, and reverse_each the same from the other
# end -- both answer the receiver too
p JSON.parse("[]").each_entry {}.size
p JSON.parse("{\"a\":1}").each_entry { |e| p e }.size
p JSON.parse("[1,2]").reverse_each {}.size
JSON.parse("[1,2,3]").reverse_each { |x| p x }
JSON.parse("{\"a\":1,\"b\":2}").reverse_each { |k, v| p k }

# a boxed value with no `each` raises where CRuby raises, rather than
# iterating zero times and handing the receiver back
begin
  JSON.parse("5").each {}
rescue => e
  p e.class
end
