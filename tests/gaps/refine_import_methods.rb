module Helpers
  def helper = :helped
end

module R
  refine String do
    import_methods Helpers
  end
end

using R
p "x".helper
