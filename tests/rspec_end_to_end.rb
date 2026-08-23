# THE rspec north star: a real `RSpec.describe` suite, run end to end by
# autorun's at_exit -- describe/it/expect, a deliberate failure with its
# report, and the summary -- byte-identical to CRuby under --seed 42.
#
# Load-path unshifts are for the CRUBY ORACLE only (rspec is not bundled
# with ruby the way minitest is); zeo resolves the same requires out of
# the vendored gems/ and runs these lines as no-ops.
%w[rspec rspec-core rspec-expectations rspec-mocks rspec-support diff-lcs].each do |g|
  $LOAD_PATH.unshift File.expand_path("../gems/#{g}/lib", __dir__)
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
# duration/load-time are the only nondeterministic bytes. Runtime defines,
# not `def` reopens: an overlay definition outranks the loaded gem's own on
# both engines regardless of load order.
#
# The FORMATTED pair, not `duration`/`load_time` themselves. Those two are
# `Struct` members, and `formatted_duration` reads its one from inside the
# same class -- a self-call zeo inlines to the slot read, which no later
# `define_method` reaches. That divergence has its own minimal repro in
# `tests/gaps/an_inlined_accessor_ignores_a_later_redefinition.rb`; stubbing
# one method further out keeps this golden measuring rspec rather than it.
RSpec::Core::Notifications::SummaryNotification.class_eval do
  define_method(:formatted_duration) { "0 seconds" }
  define_method(:formatted_load_time) { "0 seconds" }
end

# zeo declines `ripper` by policy (its front end is prism), and rspec's
# feature probe is a VERSION check, not a requirability check -- so pin the
# snippet extractor to rspec's own single-line fallback (`extract_line_at`,
# the TruffleRuby path, no ripper involved) on BOTH sides. Single-line
# expressions format identically either way; only multi-line EXPANSION
# needs ripper.
# `send`-spelled so the definition stays a RUNTIME install on both engines:
# the gem's own body is a conditional def (installed when its file loads and
# the ripper probe passes), and this must land after it, last-wins.
RSpec::Core::Formatters::SnippetExtractor.send(
  :define_singleton_method, :extract_expression_lines_at
) do |file_path, line, _max = nil|
  [extract_line_at(file_path, line)]
end
