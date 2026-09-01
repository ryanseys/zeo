# frozen_string_literal: true

# Runs the vendored upstream stringio suite (test_stringio.rb, verbatim from
# ruby/stringio at v3.2.0) against the PURE port in ../lib, under a real
# ruby. The pure tree must be required before bundler/setup runs, or the
# store's C stringio lands ahead of it on $LOAD_PATH.
$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
require "stringio"
loaded = $LOADED_FEATURES.grep(/stringio/)
unless loaded.length == 1 && loaded[0].end_with?("gems/stringio/lib/stringio.rb")
  raise "the pure tree did not win the require: #{loaded.inspect}"
end
require "bundler/setup"
require_relative "test_stringio"

# The tool/lib/core_assertions.rb helpers the suite uses, shimmed minimally.
module ZeoWarningCatcher
  class << self
    attr_accessor :buffer
  end
end
Warning.singleton_class.prepend(Module.new do
  def warn(msg, **kw)
    if (b = ZeoWarningCatcher.buffer)
      b << msg
    else
      super
    end
  end
end)

class TestStringIO
  # Capture every warning the block emits and match the whole text.
  def assert_warning(pat, msg = nil)
    ZeoWarningCatcher.buffer = buf = []
    verbose, $VERBOSE = $VERBOSE, true
    result = yield
    text = buf.join
    pat.is_a?(Regexp) ? assert_match(pat, text, msg) : assert_equal(pat, text, msg)
    result
  ensure
    $VERBOSE = verbose
    ZeoWarningCatcher.buffer = nil
  end
  alias assert_warn assert_warning

  # Runs its source in a fresh interpreter in the real harness; here the
  # one caller (test_overflow) probes pointer-width limits, which a shared
  # process answers just as well.
  def assert_separately(args, src)
    lib = File.expand_path("../lib", __dir__)
    out = IO.popen([RbConfig.ruby, "-I", lib, *args, "-e", <<~RB], &:read)
      require "test/unit/assertions"
      include Test::Unit::Assertions
      #{src}
    RB
    assert($?.success?, "assert_separately failed:\n#{out}")
  end
end

class TestStringIO
  # `all_assertions` groups labeled sub-assertions in the real harness;
  # running each block directly keeps the assertions and loses only the
  # grouping of the report.
  def all_assertions
    helper = Object.new
    helper.define_singleton_method(:for) { |_label, &block| block.call }
    yield helper
  end
end
