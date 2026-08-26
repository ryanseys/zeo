module Kernel
  MONITOR = "monitor"
  alias_method :orig_require, :require
  def require(path)
    MONITOR
    orig_require(path)
  end
end
