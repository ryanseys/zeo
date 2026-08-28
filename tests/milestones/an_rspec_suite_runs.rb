# MILESTONE: a REAL rspec suite, written the way anyone writes one, runs under
# zeo and reports what rspec reports -- examples, matchers, doubles, hooks,
# the counts and the exit status.
#
# No FAILING or PENDING example here: rspec renders those by extracting the
# source snippet, which needs `ripper`, and zeo declines ripper (prism has a
# different event model -- see docs/COMPATIBILITY.md). That half is
# `pending/rspec_reports_a_failure.rb`.
#
# Both engines read the SAME rspec, from `vendor/bundle` -- the Gemfile.lock
# set that `make install-deps` resolves. The oracle reaches it through
# bundler, zeo through `-I` on each gem's `lib/`. That makes this a true
# differential rather than a recording: neither engine has rspec of its own,
# and comparing zeo against nothing would prove nothing.
#
# rspec is deliberately NOT vendored under `gems/`. Everything there is a
# library ruby itself ships, so a `require` reaches the same code on both
# sides; vendoring rspec would make zeo answer a require ruby refuses.
#
# rspec prints a duration that cannot match across two engines, so an
# `at_exit` registered BEFORE rspec's rewrites that one line -- handlers are
# LIFO, so this sees the finished report. Everything else is compared byte for
# byte.
#
# Shapes, never versions -- see `tests/milestones.rs`.

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
