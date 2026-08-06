# minitest, vendored whole. This drives `Minitest::Test` directly rather than
# through the runner, because the runner's own report carries a random seed and
# a wall-clock time; `tests/gems/minitest_autorun.rb` covers that path.
require "minitest"
require "minitest/test"

class ExampleTest < Minitest::Test
  def setup
    @log = ["setup"]
  end

  def teardown
    @log << "teardown"
  end

  def test_passes
    assert true
    assert_equal 4, 2 + 2
    refute_equal 5, 2 + 2
    assert_nil nil
    assert_includes [1, 2, 3], 2
    assert_instance_of String, "s"
    assert_kind_of Numeric, 1
    assert_match(/ell/, "hello")
    assert_respond_to "s", :upcase
    assert_operator 2, :<, 3
    assert_empty []
    assert_in_delta 1.0, 1.001, 0.01
    assert_raises(ZeroDivisionError) { 1 / 0 }
    assert_output("hi") { print "hi" }
    assert_silent { 1 + 1 }
    assert_throws(:done) { throw :done }
  end

  def test_fails
    assert_equal 5, 2 + 2
  end

  def test_errors
    raise ArgumentError, "boom"
  end

  def test_skipped
    skip "not yet"
  end
end

def report(name)
  result = ExampleTest.new(name).run
  status = if result.passed? then "pass"
           elsif result.skipped? then "skip"
           elsif result.error? then "error"
           else "fail"
           end
  puts "#{name}: #{status} (#{result.assertions} assertions)"
  result.failures.each { |f| puts "  #{f.class}: #{f.message.lines.first.strip}" }
end

report :test_passes
report :test_fails
report :test_errors
report :test_skipped

# The suite's own bookkeeping: every subclass registers itself, and the
# runnable methods are the `test_`-prefixed ones.
p Minitest::Runnable.runnables.map(&:name)
Minitest.seed = 42
p ExampleTest.runnable_methods.sort
p ExampleTest.new(:test_passes).name

# setup/teardown really bracket the test body.
t = ExampleTest.new(:test_passes)
t.run
p t.instance_variable_get(:@log)

p Minitest::VERSION
p Minitest::Assertions.instance_method(:assert_equal).arity
