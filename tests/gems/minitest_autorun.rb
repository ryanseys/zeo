# `require "minitest/autorun"` on its own -- the way every minitest suite in
# the world starts. It used to raise `undefined method '<<' for nil` from
# `Runnable.inherited`, and needed a `require "minitest"` written ahead of it
# to work.
#
# The cause was load ORDER. autorun.rb reads:
#
#     require_relative "../minitest"
#     require_relative "spec"
#     require_relative "hell" if ENV["MT_HELL"]
#
# Zeo spliced every require it found nested inside a statement at the HEAD of
# the file, ahead of the top-level ones -- so hell.rb loaded first, and its
# `class Minitest::Test` reopen became that class's earliest body. Defining a
# `Runnable` subclass fires `inherited`, which appends to a registry that
# `Runnable`'s own body had not created yet. A nested require now splices
# where it is written, and a method-body `require_relative` at the end of the
# file it is written in -- never before the definitions around it.
#
# hell.rb sits behind a runtime-undecidable guard (`if ENV["MT_HELL"]`), so
# it is compiled in as a GATED unit and never executes here -- CRuby's order
# of events. There used to be an `.err.expected` recording the divergence
# when zeo ran every branch of a runtime conditional: hell.rb's optional
# `require "minitest/proveit"` raised LoadError in every compile and warned.
# Its absence now asserts stderr is empty, as CRuby's is.
require "minitest/autorun"

class TinyTest < Minitest::Test
  def setup
    @seen = []
  end

  def test_arithmetic
    @seen << :ran
    assert_equal 4, 2 + 2
    refute_equal 5, 2 + 2
  end

  def test_strings
    assert_match(/ell/, "hello")
  end
end

class TinySpec < Minitest::Spec
  it "describes" do
    _(1 + 1).must_equal 2
  end
end

# The registry `inherited` maintains -- the thing that used to be nil.
p Minitest::Runnable.runnables.map(&:name).include?("TinyTest")
p Minitest::Runnable.runnables.map(&:name).include?("TinySpec")
Minitest.seed = 42 # `runnable_methods` shuffles, and srands from this
p TinyTest.runnable_methods.sort
p TinySpec.runnable_methods.size

# Both halves of the gem are loaded, not just the one the umbrella names.
p defined?(Minitest::Test)
p defined?(Minitest::Spec)
p defined?(Minitest::Assertions)
p Minitest::VERSION

# The suite really runs.
result = TinyTest.new(:test_arithmetic).run
p [result.passed?, result.assertions]

# The at_exit runner autorun installs prints a random seed and a wall-clock
# time, so this golden silences the reporter and keeps the part that broke.
module Minitest
  class SummaryReporter
    def start; end
    def report; end
  end

  class ProgressReporter
    def start; end
    def record _result; end
    def report; end
  end
end
