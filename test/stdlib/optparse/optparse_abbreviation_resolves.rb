# The cycle this program builds on purpose, and the census the
# `ZEO_RT_GCCHECK=1` leg gates against. A cycle alive at exit is not a
# defect: it is a ring the program never broke. What the leg gates is a
# CHANGE to the line below.
# 
# OptionParser's own graph. A List holds its Switches and every Switch's block closes over the parser. Two parsers here, where optparse_subset.rb builds four.
#@ gccheck: cycle leak: 72 objects (Array x15, Hash x12, Proc x11, cell x9, OptionParser::OptionMap x8, OptionParser::Switch::NoArgument x5, OptionParser::List x4, OptionParser::Switch::OptionalArgument x4, OptionParser x2, OptionParser::Switch::RequiredArgument x2)
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
__END__
:v
OptionParser::AmbiguousOption: ambiguous option: --fooba
