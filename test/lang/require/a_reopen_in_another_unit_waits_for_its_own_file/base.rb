class K
  def base = :base
end

require_relative "extra" if ENV["LOAD_EXTRA"]
