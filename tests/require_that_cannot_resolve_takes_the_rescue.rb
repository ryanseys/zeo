# The i18n/excon shape: an optional accelerator behind `begin; require "x";
# <definitions>; rescue LoadError; <fallback>`. The feature does not exist, so
# ruby raises at the require and only the fallback ever runs.

module Codec
  begin
    require "an_accelerator_that_is_not_installed"

    class Backend
      def self.name
        "native"
      end
    end
  rescue LoadError
    class Backend
      def self.name
        "pure ruby"
      end
    end
  end
end

puts Codec::Backend.name
puts defined?(Codec::Backend)
