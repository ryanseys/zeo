# Issue #408. Two coordinated changes here:
#
# 1. Port the per-block-body str-concat collector from codegen.rb
#    to spinel_analyze.rb. The original #408 fix (commit 9ca01d77)
#    added it to spinel_codegen.rb's narrow_param_hash_types_from_body_writes,
#    but the analyze/codegen split (PR #416) moved the
#    inference passes to spinel_analyze.rb; the codegen-side copy
#    no longer runs. The analyze-side copy now harvests the
#    same str-concat hints from `pname.each do |k, v|` block
#    bodies that the codegen copy used to.
#
# 2. Option B "weak default" from Ori's issue body: when no
#    concrete signals at all (no `[]=` writes, no str-concat
#    hints from the each-block) and the current param type is
#    `poly_poly_hash` / `sym_poly_hash` / `str_poly_hash`, AND
#    the body has at least one `pname.each do |k, v|` shape on
#    the param, default to `str_str_hash`. Unsound in general
#    but covers the dominant string-keyed-Hash shape in
#    Rails/Tep-like code where the body just iterates without
#    inserting back.
#
# Out of scope for this fix (still #424): when the each-body
# calls a sibling cmeth with k or v as an arg, propagating that
# k/v narrowing into the cmeth's param widening needs a
# different scope-handling shape than the inference pipeline
# currently uses (the targeted block-scope push tried for #424
# breaks issue207's static-fold convergence). #424 stays open
# with a comment explaining the residual.

# Shape 1: str-concat hint -- the existing path. The body
# concatenates `+ k + "=" + v` into `out`; both k and v
# contribute "string" to the hash variant inference.
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

# Shape 2: weak default -- no str-concat, just each|k,v|. The
# body delegates to sibling cmeths whose param types haven't
# been widened yet; option B sees the each|k,v| presence and
# defaults the param to str_str_hash so the body compiles
# under that assumption.
class HashDumper
  def initialize
    @pairs = []
  end
  attr_reader :pairs

  def collect(h)
    h.each do |k, v|
      @pairs << k
      @pairs << v
    end
  end
end

d = HashDumper.new
d.collect({"name" => "ada", "role" => "engineer"})
puts d.pairs.length.to_s   # 4
puts d.pairs[0]            # name
puts d.pairs[1]            # ada
puts d.pairs[2]            # role
puts d.pairs[3]            # engineer
