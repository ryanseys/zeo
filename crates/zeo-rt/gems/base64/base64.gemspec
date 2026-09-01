# The pure-Ruby gem tier's `base64`: the official gem's own lib, vendored
# verbatim at the locked release. A build that omits `ext-base64` serves
# this tree; one that carries the Rust ext answers the require natively and
# never reaches it -- the dual-build switch.
Gem::Specification.new do |s|
  s.name = "base64"
  s.version = "0.3.0"
  s.summary = "Support for encoding and decoding binary data using a Base64 representation."
  s.require_paths = ["lib"]
end
