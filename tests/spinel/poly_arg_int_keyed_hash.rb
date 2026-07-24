# `if status.is_a?(Integer)` inside a method where status is
# poly-typed (the method's caller-union widened it). The then-arm
# uses status as the key into an Int-keyed hash. Pre-fix the
# IntStrHash dispatch (`[]`, has_key?, fetch) compiled the key
# arg via compile_arg0 / compile_expr without unboxing the poly
# value, and the C call passed sp_RbVal to a mrb_int param,
# tripping the C compile with int-from-pointer.
#
# IntStrHash's three key-receiving arms now route through
# compile_arg0_as_int (or explicit `.v.i` unbox in fetch), which
# detects a poly arg and emits `(arg).v.i`. At runtime the is_a?
# guard ensures the tag is SP_TAG_INT so the unbox reads the
# intended integer value; with no guard the unbox would still
# compile but look up a meaningless key (no crash — union
# type-pun is defined, just non-meaningful).
#
# The broader is_a? narrowing for other Int-context use sites
# (method call args expecting mrb_int, arithmetic ops, etc.)
# remains a separate concern.

def lookup(status)
  table = { 200 => "OK", 404 => "NotFound" }
  if status.is_a?(Integer)
    return table.fetch(status, "x")
  end
  "no"
end

puts lookup(200)
puts lookup(404)
puts lookup("nope")

def has_status?(status)
  table = { 200 => "OK" }
  if status.is_a?(Integer)
    return table.has_key?(status)
  end
  false
end

puts has_status?(200)
puts has_status?("nope")
