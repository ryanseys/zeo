module Kernel
  alias_method :zeo_orig_frozen, :frozen?
  private :zeo_orig_frozen
end
