# rack's spec_utils.rb, run for real: the driver requires the vendored test file
# (helper.rb loads rack itself and minitest-global_expectations' autorun),
# the at_exit runner executes every spec under the seeded shuffle, and the
# stubbed statistics line removes the wall clock. See tests/gemtests.rs for
# the load roots and working directory this compiles and runs under.
ENV["MT_CPU"] = "1"
require "spec_utils"
module Minitest
  class SummaryReporter
    def statistics = "Finished in 0.00s"
  end
end
