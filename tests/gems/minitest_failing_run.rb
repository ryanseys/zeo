# The FAILING half of the runner's report, end to end: an assertion
# failure (Expected/Actual block + [file:line] location), a raised error
# (class + message + backtrace line), and a skip -- plus the exit status
# the autorun at_exit handler sets, which the e2e CLI suite asserts. The
# [file:line] locations stay relative because minitest strips the
# Dir.pwd prefix and both zeo and the oracle run from the repo root.
#
# Same determinism scaffolding as minitest_seeded_run.rb.
ENV["MT_CPU"] = "1"
require "minitest/autorun"

class ReportShapesTest < Minitest::Test
  def test_passes = assert_equal 1, 1

  def test_fails
    assert_equal 1, 2
  end

  def test_errors
    raise ArgumentError, "deliberate boom"
  end

  def test_skips
    skip "not today"
  end
end

module Minitest
  class SummaryReporter
    def statistics = "Finished in 0.00s"
  end
end
