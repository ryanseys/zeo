# OptionParser resolves a unique long-option PREFIX (`--verb` finds
# `--verbose`) and raises AmbiguousOption when two candidates share it.
#
# Both rows once answered InvalidOption, because optparse collects its
# candidates through `method(:each).to_proc` and that proc dropped its
# block -- see a_method_to_proc_carries_its_block.rb for the primitive.
require "optparse"
got = nil
OptionParser.new { |o| o.on("--verbose") { got = :v } }.parse(["--verb"])
p got
begin
  o = OptionParser.new do |op|
    op.on("--foobar") { }
    op.on("--foobaz") { }
  end
  o.parse(["--fooba"])
rescue OptionParser::AmbiguousOption => e
  puts "#{e.class}: #{e.message}"
end
