# The SPEC DSL end to end: `describe` mints a runtime Minitest::Spec
# subclass, `it` installs test methods from compiled blocks, and the
# must_*/wont_* expectations run through methods minitest defines via a
# string `class_eval` (the eval VM's case/when). The whole flow rides the
# dynamic-self class-method twins: `Runnable.run`'s `self` is the minted
# spec class, so its `new`, its `runnable_methods`, and its reported name
# are the receiver's own. This was 0 runs before those landed.
#
# Same determinism scaffolding as minitest_seeded_run.rb: MT_CPU pins the
# executor width, the .args sidecar seeds both zeo and the oracle, and the
# statistics stub replaces the wall-clock line.
ENV["MT_CPU"] = "1"
require "minitest/autorun"

describe "arithmetic" do
  it "adds" do
    _(1 + 1).must_equal 2
  end

  it "compares" do
    _(3).must_be :>, 2
  end

  it "collects" do
    _([1, 2, 3].map { |x| x * 2 }).must_equal [2, 4, 6]
  end
end

describe "strings" do
  it "matches" do
    _("abc").must_match(/b/)
  end

  it "refutes" do
    _("abc").wont_include "z"
  end
end

module Minitest
  class SummaryReporter
    def statistics = "Finished in 0.00s"
  end
end
