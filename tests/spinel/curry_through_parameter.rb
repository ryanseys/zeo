# Whether an application saturates a curry is decided statically when the base
# proc's arity can be traced. A curry that arrives through a parameter, a
# container or an untyped slot cannot be traced, and answering "not yet
# saturated" for those meant a method taking a curried Proc returned a Proc
# where CRuby returns the value (#4068). Saturation is a run-time property of
# the accumulator, so it is read there.
JOIN = ->(a, b) { "#{a}/#{b}" }

def apply_final(rule)
  rule.call("x")
end

curried = JOIN.curry["nickname"]
p curried.call("x")
p apply_final(curried)
p apply_final(curried).class

# a partial application through the same parameter still answers a Proc
TRIPLE = ->(a, b, c) { "#{a}#{b}#{c}" }
def apply_one(rule)
  rule.call("2")
end
part = TRIPLE.curry["1"]
p apply_one(part).class
p apply_one(part).call("3")

# through a container, and through a method's return
procs = [JOIN.curry["k"]]
p procs[0].call("v")

def fetch_rule
  JOIN.curry["m"]
end
p fetch_rule.call("n")

# the statically traceable forms are unchanged
p JOIN.curry["a"]["b"]
p JOIN.curry.call("a", "b")
p ->() { 5 }.curry.call
