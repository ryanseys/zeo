# #560 (Sam Ruby). poly_poly_hash variant gained fetch + dup
# dispatch arms (mirror of #551's str_poly_hash / sym_poly_hash
# work). When two writers to the same ivar use keys of
# incompatible inferred types (int-defaulted param + sym
# literal), spinel widens the storage to poly_poly_hash; the
# read side then needs the dispatch cells to lower correctly.

module M
  @h = {}

  def self.write1(k, v)
    @h[k] = v
  end

  def self.write2(v)
    @h[:special] = v
  end

  def self.read(k)
    @h.fetch(k, nil)
  end
end

M.write2("hello")
puts M.read(:special)
puts M.read(:missing).nil?
