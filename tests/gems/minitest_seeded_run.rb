# A REAL seeded `Minitest.run`, end to end: autorun's at_exit fires, the
# suites shuffle under `--seed 42` (the .args sidecar hands it to the zeo
# binary and the CRuby oracle alike), the progress dots print, and the
# summary counts land. This is the load-bearing probe for the gem-test tier:
# zeo's MT19937 and CRuby-exact Fisher-Yates shuffle must order
# `runnable_methods` element-for-element like CRuby's own, or no real
# suite's output can ever match byte-for-byte.
#
# `MT_CPU=1` pins the parallel executor's size before minitest loads (it
# reads the knob at document position); the stubbed
# `SummaryReporter#statistics` replaces the wall-clock line -- the one
# nondeterministic thing the runner prints for a passing suite.
ENV["MT_CPU"] = "1"
require "minitest/autorun"

class AlphaTest < Minitest::Test
  def test_a = assert_equal 1, 1
  def test_b = assert_equal 2, 2
  def test_c = assert_includes [1, 2], 2
  def test_d = refute_nil :x
end

class BetaTest < Minitest::Test
  def test_e = assert_match(/b/, "abc")
  def test_f = assert_equal "x", "x"
end

# The shuffle itself, made visible ahead of the run: `runnable_methods`
# under a pinned seed is `methods.sort.shuffle` after `srand seed`.
Minitest.seed = 42
p AlphaTest.runnable_methods
Minitest.seed = 42
p BetaTest.runnable_methods

module Minitest
  class SummaryReporter
    def statistics = "Finished in 0.00s"
  end
end
