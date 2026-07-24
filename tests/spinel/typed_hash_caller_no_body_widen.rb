# #556 (Ori Pekelman). Smoke test for the move of
# infer_hash_param_from_body out of the iterative inference
# loop. The full bug only triggers in tep's framework
# surface (where iter 0 leaves the param's ivar slot
# untyped, the body-usage widening fires, then
# unify_call_types pulls it to poly against the actual
# typed caller). Reductions to a standalone script don't
# reproduce -- iter 0 already pins the ivar via the
# initial-literal observation. Documenting both here:
# this fixture exercises the "typed caller + body[]
# usage" shape end-to-end so a future regression that
# poisons the typed-caller path would be caught even if
# the iter-0 timing is more permissive than tep's.
#
# Verified against tep@9ca09fd: pre-fix
# `sp_tep_mustache_m_ivars(sp_StrStrHash *, sp_RbVal)`,
# post-fix `sp_tep_mustache_m_ivars(sp_StrStrHash *, sp_StrStrHash *)`,
# caller drops the sp_box_obj wrap.

class Request
  attr_accessor :ivars
  def initialize
    @ivars = {"k" => "v"}
    @ivars.delete("k")
  end
end

def lookup_typed(h, key)
  h[key].to_s
end

def forward_to_typed(locals, ivars)
  out = ""
  out += lookup_typed(ivars, "name")
  out += "/"
  out += lookup_typed(ivars, "count")
  out += "/"
  out += ivars["raw"].to_s
  out
end

req = Request.new
req.ivars["name"] = "alice"
req.ivars["count"] = "5"
req.ivars["raw"] = "<i>I</i>"
puts forward_to_typed({"unused" => "x"}, req.ivars)
