# MILESTONE: a REAL rspec suite, written the way anyone writes one, runs under
# zeo and reports what rspec reports -- passes, failures, pending, doubles,
# hooks, the counts and the exit status.
#
# Both engines load the SAME rspec, the copy zeo vendors, put on the load path
# below. That makes this a true differential rather than a recording: the
# reference ruby has no rspec installed of its own, and comparing zeo against
# nothing would prove nothing.
#
# Every failing example used to die IN THE FORMATTER. rspec asks
# `RubyFeatures.ripper_supported?`, which upstream infers from the engine name
# -- it opts out only for rbx, jruby and truffleruby -- and then does a bare
# `require "ripper"` with no rescue. zeo embeds prism rather than CRuby's
# parse.y and declines that require. rspec already has a ripper-less branch
# (`NoSnippetExtractor`, the path a JRuby user gets); the vendored copy now
# PROBES for ripper instead of guessing, so that branch is reachable. See
# `gems/UPSTREAM.md`.
#
# rspec prints a duration that cannot match across two engines, so an
# `at_exit` registered BEFORE rspec's rewrites that one line -- handlers are
# LIFO, so this sees the finished report. Everything else is compared byte for
# byte.
#
# Shapes, never versions -- see `tests/milestones.rs`.

%w[rspec-core rspec-support rspec-expectations rspec-mocks diff-lcs].each do |gem|
  $LOAD_PATH.unshift(File.expand_path("../../gems/#{gem}/lib", __dir__))
end

require "stringio"
real_stdout = $stdout
$stdout = StringIO.new
at_exit do
  report = $stdout.string
  $stdout = real_stdout
  print report.gsub(/Finished in .*/, "Finished in <time>")
end

require "rspec/autorun"

RSpec.describe "arithmetic" do
  let(:values) { [1, 2, 3] }
  subject { values.sum }

  it "sums" do
    expect(subject).to eq(6)
  end

  it "maps" do
    expect(values.map { |v| v * 2 }).to eq([2, 4, 6])
  end

  it "raises" do
    expect { 1 / 0 }.to raise_error(ZeroDivisionError)
  end

  it "reports a failure" do
    expect(1).to eq(2)
  end

  it "is pending" do
    pending "not yet"
    raise "still broken"
  end

  context "with matchers" do
    it "matches strings" do
      expect("hello").to match(/ell/)
      expect("hello").to start_with("he")
      expect(%w[a b]).to include("a")
      expect(values).to all(be_a(Integer))
      expect(values).to contain_exactly(3, 2, 1)
    end

    it "negates" do
      expect(values).not_to be_empty
      expect(values.first).not_to be_nil
      expect(values).to have_attributes(size: 3)
    end
  end

  describe "hooks" do
    before(:each) { @seen = "before" }
    after(:each)  { @seen = nil }

    it "sees the hook" do
      expect(@seen).to eq("before")
    end
  end

  describe "doubles" do
    it "stubs and verifies" do
      thing = double("thing", size: 2)
      expect(thing.size).to eq(2)

      spy = double("spy")
      allow(spy).to receive(:call).and_return(:ok)
      expect(spy.call).to eq(:ok)
      expect(spy).to have_received(:call)
    end
  end
end
