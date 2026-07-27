# A bare `require "erb"` fails to compile: the generated Rust references
# `self.frozen_string` on the ERB struct, but that field was never generated
# on it (only `__frozen`, `src`, `filename`, `lineno`, `_init`, and two
# others exist) -- a codegen/struct-layout mismatch for the ERB class.
require "erb"
p ERB.respond_to?(:new)
