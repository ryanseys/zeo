$inner_runs = ($inner_runs || 0) + 1

module RuntimeInner
  def self.n = 7

  def self.boom = raise("from inner")
end
