# The poly dispatch records what the BUILTIN surface alone would answer, to
# shape its builtin arm. That re-derivation is defined as "if no user class
# owned the name", but the helper it went through still took the union with a
# user return -- so the recorded type came back poly and the builtin arm
# assigned sp_poly_values()'s raw sp_PolyArray * into the boxed slot the user
# arms need, and the C build stopped. It takes one user definition to see it.
class Flash
  def initialize
    @h = { "a" => "b" }
  end

  def values = @h.values
  def keys = @h.keys
  def to_a = @h.to_a
end

def vals(x) = x.values
def ks(x) = x.keys
def entries(x) = x.to_a

p vals(Flash.new)
p vals({ a: 1 })
p ks(Flash.new)
p ks({ "c" => 2 })
p entries(Flash.new)
p entries({ "c" => 2 })
