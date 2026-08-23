# The pure-Ruby half, which pulls in the compiled half exactly as a real gem
# does. `require "nativelib/nativelib"` is what `create_makefile` names.
require "nativelib/nativelib"

module Nativelib
  VERSION = "1.0.0"
end
