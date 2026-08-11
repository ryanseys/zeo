# THE rspec north star: a real `RSpec.describe` suite, run end to end by
# autorun's at_exit -- describe/it/expect, a deliberate failure with its
# report, and the summary -- byte-identical to CRuby under --seed 42.
#
# Load-path unshifts are for the CRUBY ORACLE only (rspec is not bundled
# with ruby the way minitest is); zeo resolves the same requires out of
# the vendored gems/ and runs these lines as no-ops.
%w[rspec rspec-core rspec-expectations rspec-mocks rspec-support diff-lcs].each do |g|
  $LOAD_PATH.unshift File.expand_path("../../gems/#{g}/lib", __dir__)
end
require "rspec/autorun"

RSpec.describe "zeo end to end" do
  it "adds" do
    expect(1 + 1).to eq(2)
  end
  it "compares strings" do
    expect("ab" + "c").to eq("abc")
  end
  it "fails on purpose" do
    expect([1, 2, 3]).to include(4)
  end
end

# Wall-clock stubs, the same trick the minitest goldens use: the summary's
# duration/load-time are the only nondeterministic bytes.
module RSpec
  module Core
    module Notifications
      class SummaryNotification
        def duration = 0.0
        def load_time = 0.0
      end
    end
  end
end
