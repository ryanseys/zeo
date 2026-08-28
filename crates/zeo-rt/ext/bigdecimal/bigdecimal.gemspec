# The Ruby half of `bigdecimal` -- vendored verbatim from the bigdecimal
# 4.1.2 gem (ruby 4.0.5's bundled version). In 4.x this is most of the gem:
# `**`/`power`, `sqrt`, `BigMath`, and the `to_d` family are all Ruby; the
# statically linked `ext-bigdecimal` module in zeo-rt is the same C slice
# upstream keeps native (arithmetic, rounding, mode state).
Gem::Specification.new do |s|
  s.name = "bigdecimal"
  s.version = "4.1.2"
  s.summary = "Arbitrary-precision decimal floating-point arithmetic."
  s.require_paths = ["lib"]
end
