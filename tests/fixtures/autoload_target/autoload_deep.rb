# The NESTED-ask target: `defined?(Nested::Deep::Inner)` has to load this to
# answer, because resolving the head is a real constant read.
$autoload_deep_loaded = true

module Nested
  module Deep
    module Inner; end
  end
end
