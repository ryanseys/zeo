# benchmark is a pure-Ruby default gem (no C extension) that isn't vendored
# under gems/ -- `require "benchmark"` raises LoadError.
require "benchmark"
p Benchmark.respond_to?(:measure)
