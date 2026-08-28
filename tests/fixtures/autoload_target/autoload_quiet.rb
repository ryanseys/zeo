# The target a BARE `defined?` must NOT load.
$autoload_quiet_loaded = true

module Untouched
  class Quiet; end
end
