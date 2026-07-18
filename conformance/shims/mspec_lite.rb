# mspec_lite: a tiny, AOT-compilable re-implementation of the slice of mspec's
# DSL that ruby/spec's `language/` (and simple `core/`) files use. It runs every
# `describe`/`it` immediately as the spec file is required, tallies results, and
# the driver prints a machine-readable summary the conformance harness parses:
#
#   MSPEC_LITE examples=<n> failures=<f> errors=<e>
#
# No `instance_eval`/`instance_exec` is used (blocks run as plain closures), so
# a `before` sharing state with an `it` does so through the top-level object's
# ivars rather than a per-example object -- adequate for the language specs,
# which lean on locals. Unsupported corners (mocks, shared examples) raise
# `MSpecLiteUnsupported`, counting the example as an error, never a silent pass.

class MSpecLiteUnsupported < StandardError; end
class MSpecExpectationNotMet < StandardError; end

module MSpecLite
  @examples = 0
  @failures = 0
  @errors = 0
  @stack = []
  @current = ""

  class << self
    def examples; @examples; end
    def failures; @failures; end
    def errors; @errors; end

    def push_group(desc)
      parent = @stack.empty? ? "" : @stack.last[:desc]
      full = parent.empty? ? desc.to_s : "#{parent} #{desc}"
      @stack.push({ desc: full, befores: [], afters: [] })
    end

    def pop_group
      @stack.pop
    end

    def group
      @stack.last
    end

    def add_before(blk)
      @stack.last[:befores].push(blk) unless @stack.empty?
    end

    def add_after(blk)
      @stack.last[:afters].push(blk) unless @stack.empty?
    end

    # Every before hook from the enclosing describe chain, outermost first.
    def all_befores
      @stack.flat_map { |g| g[:befores] }
    end

    def all_afters
      @stack.flat_map { |g| g[:afters] }
    end

    def run_example(desc, blk)
      @examples += 1
      @current = @stack.empty? ? desc.to_s : "#{@stack.last[:desc]} #{desc}"
      befores = all_befores
      afters = all_afters
      begin
        befores.each { |b| b.call }
        blk.call
      rescue MSpecExpectationNotMet => e
        @failures += 1
        $stderr.puts("FAIL: #{@current}: #{e.message}")
      rescue MSpecLiteUnsupported => e
        @errors += 1
        $stderr.puts("UNSUPPORTED: #{@current}: #{e.message}")
      rescue => e
        @errors += 1
        $stderr.puts("ERROR: #{@current}: #{e.class}: #{e.message}")
      ensure
        afters.each { |a| begin; a.call; rescue; end }
      end
    end

    def assert(ok, msg)
      raise MSpecExpectationNotMet, msg unless ok
    end

    def summary_line
      "MSPEC_LITE examples=#{@examples} failures=#{@failures} errors=#{@errors}"
    end

    def version_parts(str)
      str.split(".").map { |p| p.to_i }
    end

    # -1 / 0 / 1 comparing dotted version strings component-wise.
    def cmp_versions(a, b)
      pa = version_parts(a)
      pb = version_parts(b)
      n = pa.length > pb.length ? pa.length : pb.length
      i = 0
      while i < n
        x = i < pa.length ? pa[i] : 0
        y = i < pb.length ? pb[i] : 0
        return -1 if x < y
        return 1 if x > y
        i += 1
      end
      0
    end

    # mspec's `ruby_version_is "a" ... "b"` / `"a" .. "b"` / bare `"a"` (min).
    def version_matches?(range)
      if range.is_a?(String)
        cmp_versions(RUBY_VERSION, range) >= 0
      elsif range.is_a?(Range)
        lo = range.begin
        hi = range.end
        ge = lo.nil? || cmp_versions(RUBY_VERSION, lo) >= 0
        return ge if hi.nil?
        cmp = cmp_versions(RUBY_VERSION, hi)
        ge && (range.exclude_end? ? cmp < 0 : cmp <= 0)
      else
        true
      end
    end
  end
end

# --- the group / example DSL (bare calls resolve to these top-level privates) --

def describe(desc, *_ignored, &blk)
  return unless blk
  MSpecLite.push_group(desc)
  begin
    blk.call
  ensure
    MSpecLite.pop_group
  end
end

def context(desc, *rest, &blk)
  describe(desc, *rest, &blk)
end

def it(desc = "example", *_ignored, &blk)
  return unless blk
  MSpecLite.run_example(desc, blk)
end

def specify(desc = "specify", &blk)
  it(desc, &blk)
end

def before(_scope = :each, &blk)
  MSpecLite.add_before(blk) if blk
end

def after(_scope = :each, &blk)
  MSpecLite.add_after(blk) if blk
end

# Version / platform guards.
def ruby_version_is(range)
  yield if block_given? && MSpecLite.version_matches?(range)
end

def ruby_bug(*)
  # A `ruby_bug` guard marks a spec that fails on buggy versions; run it.
  yield if block_given?
end

def platform_is(*)
  yield if block_given?
end

def platform_is_not(*)
  yield if block_given?
end

def guard(*)
  yield if block_given?
end

def guard_not(*)
  yield if block_given?
end

# Shared examples / mocks are out of scope -- fail loudly.
def it_behaves_like(*)
  raise MSpecLiteUnsupported, "it_behaves_like"
end

def it_should_behave_like(*)
  raise MSpecLiteUnsupported, "it_should_behave_like"
end

# --- expectations: `obj.should` / `obj.should_not` and the matchers ----------

# The comparison proxy `should`/`should_not` return when given no matcher, so
# `x.should == y` becomes `x.should.==(y)`.
class MSpecShould
  def initialize(obj, negated)
    @obj = obj
    @neg = negated
  end

  def check(ok, desc, other)
    want = @neg ? !ok : ok
    MSpecLite.assert(want, "expected #{@obj.inspect} #{@neg ? 'not ' : ''}#{desc} #{other.inspect}")
  end

  def ==(other); check(@obj == other, "==", other); end
  def !=(other); check(@obj != other, "!=", other); end
  def <(other); check(@obj < other, "<", other); end
  def >(other); check(@obj > other, ">", other); end
  def <=(other); check(@obj <= other, "<=", other); end
  def >=(other); check(@obj >= other, ">=", other); end
  def =~(other); check((@obj =~ other) ? true : false, "=~", other); end
end

class Object
  def should(matcher = nil)
    if matcher.nil?
      MSpecShould.new(self, false)
    else
      MSpecLite.assert(matcher.matches?(self), matcher.failure_message(self, false))
      self
    end
  end

  def should_not(matcher = nil)
    if matcher.nil?
      MSpecShould.new(self, true)
    else
      MSpecLite.assert(!matcher.matches?(self), matcher.failure_message(self, true))
      self
    end
  end
end

class MSpecMatcher
  def failure_message(obj, negated)
    "expected #{obj.inspect} #{negated ? 'not ' : ''}to #{describe_self}"
  end
  def describe_self; "match"; end
end

class BeTrueMatcher < MSpecMatcher
  def matches?(o); o == true; end
  def describe_self; "be true"; end
end
class BeFalseMatcher < MSpecMatcher
  def matches?(o); o == false; end
  def describe_self; "be false"; end
end
class BeNilMatcher < MSpecMatcher
  def matches?(o); o.nil?; end
  def describe_self; "be nil"; end
end
class BeKindOfMatcher < MSpecMatcher
  def initialize(k); @k = k; end
  def matches?(o); o.is_a?(@k); end
  def describe_self; "be a kind of #{@k}"; end
end
class BeAnInstanceOfMatcher < MSpecMatcher
  def initialize(k); @k = k; end
  def matches?(o); o.instance_of?(@k); end
  def describe_self; "be an instance of #{@k}"; end
end
class EqlMatcher < MSpecMatcher
  def initialize(v); @v = v; end
  def matches?(o); o.eql?(@v); end
  def describe_self; "eql #{@v.inspect}"; end
end
class EqualMatcher < MSpecMatcher
  def initialize(v); @v = v; end
  def matches?(o); o.equal?(@v); end
  def describe_self; "equal #{@v.inspect}"; end
end
class BeEmptyMatcher < MSpecMatcher
  def matches?(o); o.empty?; end
  def describe_self; "be empty"; end
end
class IncludeMatcher < MSpecMatcher
  def initialize(vs); @vs = vs; end
  def matches?(o); @vs.all? { |v| o.include?(v) }; end
  def describe_self; "include #{@vs.inspect}"; end
end
class BeCloseMatcher < MSpecMatcher
  def initialize(exp, tol); @exp = exp; @tol = tol; end
  def matches?(o); (o - @exp).abs <= @tol; end
  def describe_self; "be close to #{@exp}"; end
end
class RaiseErrorMatcher < MSpecMatcher
  def initialize(klass, msg); @klass = klass; @msg = msg; end
  def matches?(subject)
    begin
      subject.call
      false
    rescue Exception => e
      klass_ok = @klass.nil? || e.is_a?(@klass)
      msg_ok = case @msg
               when nil then true
               when Regexp then (e.message =~ @msg ? true : false)
               else e.message == @msg
               end
      klass_ok && msg_ok
    end
  end
  def describe_self; "raise #{@klass}"; end
end

def be_true; BeTrueMatcher.new; end
def be_false; BeFalseMatcher.new; end
def be_nil; BeNilMatcher.new; end
def be_kind_of(k); BeKindOfMatcher.new(k); end
def be_an_instance_of(k); BeAnInstanceOfMatcher.new(k); end
def eql(v); EqlMatcher.new(v); end
def equal(v); EqualMatcher.new(v); end
def be_empty; BeEmptyMatcher.new; end
def include(*vs); IncludeMatcher.new(vs); end
def be_close(exp, tol = 0.00003); BeCloseMatcher.new(exp, tol); end
def raise_error(klass = nil, msg = nil); RaiseErrorMatcher.new(klass, msg); end
