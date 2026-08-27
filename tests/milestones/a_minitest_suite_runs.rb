# MILESTONE: a REAL minitest suite, written the way anyone writes one, runs
# under zeo and reports what ruby reports -- passes, failures, errors, skips,
# assertion counts and the exit status.
#
# `--seed 42` is pinned because minitest randomizes test order, and it is
# passed the way ruby passes it: option parsing stops at the file name, so
# `zeo test.rb --seed 42` hands all of it to ARGV. That only became possible
# when zeo adopted ruby's rule; before it, `--seed` was "invalid option".
#
# Shapes, never versions -- see `tests/milestones.rs`. Timings are the one
# thing that cannot match, so nothing here prints one.

# Minitest prints a duration and a random seed, and neither can match across
# two engines. They are normalized HERE rather than by the harness: an
# `at_exit` registered BEFORE `minitest/autorun`'s runs AFTER it (handlers are
# LIFO), so this sees the finished report and rewrites exactly two lines.
# Everything else -- every dot, failure, error, skip and count -- is compared
# byte for byte.
require "stringio"
real_stdout = $stdout
$stdout = StringIO.new
at_exit do
  report = $stdout.string
  $stdout = real_stdout
  print report
    .gsub(/Finished in .*/, "Finished in <time>")
    .gsub(/Run options: --seed \d+/, "Run options: --seed <n>")
end

require "minitest/autorun"

class HooksTest < Minitest::Test
  ORDER = []
  def setup;    ORDER << "setup"; end
  def teardown; ORDER << "teardown"; end
  def test_a;   ORDER << "a"; assert true; end
  def test_b;   ORDER << "b"; assert true; end
  Minitest.after_run { puts "order=#{ORDER.inspect}" }
end

class FailuresTest < Minitest::Test
  def test_fails
    assert_equal 1, 2, "one is not two"
  end

  def test_errors
    raise ArgumentError, "boom"
  end

  def test_skipped
    skip "not today"
  end
end

class AssertionsTest < Minitest::Test
  def test_variety
    assert_operator 3, :>, 2
    assert_instance_of String, "x"
    assert_kind_of Numeric, 1
    assert_respond_to [], :each
    assert_same :a, :a
    assert_in_delta 1.0, 1.001, 0.01
    assert_output(/hi/) { print "hi" }
    assert_silent {}
    assert_throws(:done) { throw :done }
    assert_predicate [], :empty?
    assert_raises(ZeroDivisionError) { 1 / 0 }
    assert_match(/ell/, "hello")
    assert_includes %w[a b], "a"
    refute_empty [1]
    refute_nil 1
    assert_empty []
    assert_nil nil
  end
end

describe "the spec style" do
  it "expects" do
    _(1 + 1).must_equal 2
  end

  it "negates" do
    _([]).wont_be :any?
  end
end
