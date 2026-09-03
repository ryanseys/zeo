require_relative "deep/inner"
LOADED_TOP_FILE = __FILE__
$top_runs = ($top_runs || 0) + 1

module RuntimeTop
  def self.hi = "top #{RuntimeInner.n}"
end

class Raiser
  def boom = RuntimeInner.boom
end
