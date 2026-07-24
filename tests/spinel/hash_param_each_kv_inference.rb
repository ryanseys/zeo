# Issue #408 (followup to #397). Hash params iterated solely via
# `each do |k, v|` got no signal through the existing
# `narrow_param_hash_types_from_body_writes` pass -- that one only
# fires for `pname[k] = v` write-back patterns. A formatter that
# read-only-iterates the hash, like the canonical
# `Tep::Json.encode_object` shape:
#
#   def self.encode(h)
#     out = ""
#     h.each do |k, v|
#       out = out + k + "=" + v
#     end
#     out
#   end
#
# left h widened to whichever poly variant an earlier call site
# pinned, so subsequent string-concat sites tripped the C compiler
# with `passing 'sp_RbVal' to const char *`.
#
# Fix: extend `infer_param_hash_from_writes` with a parallel
# collector that walks `pname.each do |k, v|` block bodies and
# harvests type signals from how `k` and `v` participate in
# `+`-chains. A chain with both a string literal leaf and a
# k_pname / v_pname leaf is treated as evidence the corresponding
# side is string-typed, narrowing the hash variant accordingly.
#
# The test exercises two shapes:
#   - module class method with str_str_hash inputs at two
#     differently-keyed call sites (the canonical Tep shape),
#   - class instance method with the same body shape (the path
#     that already worked, kept for parity).

module Joiner
  def self.encode(h)
    out = ""
    first = true
    h.each do |k, v|
      if !first
        out = out + ","
      end
      first = false
      out = out + k + "=" + v
    end
    out
  end
end

puts Joiner.encode({"a" => "1", "b" => "2"})    # a=1,b=2
puts Joiner.encode({"x" => "y"})                # x=y

class Cookie
  def serialize(opts)
    out = ""
    first = true
    opts.each do |k, v|
      if !first
        out = out + ";"
      end
      first = false
      out = out + k + "=" + v
    end
    out
  end
end

c = Cookie.new
puts c.serialize({"path" => "/", "domain" => "example.com"})
