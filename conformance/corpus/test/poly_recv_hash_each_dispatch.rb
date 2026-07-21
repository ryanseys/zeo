# Hash#each on a poly-typed receiver (sp_RbVal) was silently
# iterating zero times: compile_each_block's `rt == "poly"` arm
# emitted a cls_id-keyed length / per-iteration table covering
# Range and the six Array variants but no Hash variants. The
# loop's `_t2_len` defaulted to 0 and the block never ran.
#
# Fix: codegen `scan_for_container_typedef_usage` arms every
# Hash typedef when any `recv.each { ... }` is present
# (conservative -- the recv may resolve to poly at the use site
# even when the static analyzer's first pass sees a narrower
# type), and the poly-each emit pass now includes 8 hash arms:
# `_t2_len` reads `h->len`, the per-iteration value packs the
# (key, value) as a 2-element PolyArray so the existing
# block-param splat delivers `|k, v|`. Issue #603.

module M
  def self.iter(hash)
    n = 0
    hash.each { |_k, _v| n += 1 }
    n
  end
end

# Two heterogeneous call sites force the param to sp_RbVal so
# the poly-each emit path is exercised on both shapes.
puts "a=#{M.iter({ a: 1, b: 2 })}"     # sym_int_hash, expect 2
puts "b=#{M.iter({ "x" => "y" })}"     # str_str_hash, expect 1

# Same surface with int-keyed hash + str values, an empty hash,
# and a single-pair str_int_hash. The third call drives the
# per-iteration block param so we verify keys/values arrive.
def m3(h)
  out = ""
  h.each { |k, v| out += k.to_s + "=" + v.to_s + ";" }
  out
end

puts m3({1 => "one", 2 => "two"})        # int_str_hash
puts m3({})                              # empty
puts m3({"k" => 42})                     # str_int_hash
