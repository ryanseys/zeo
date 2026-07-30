# `require "benchmark"` -- the vendored gem loads and `Benchmark.measure`
# answers.
require "benchmark"
p Benchmark.respond_to?(:measure)
