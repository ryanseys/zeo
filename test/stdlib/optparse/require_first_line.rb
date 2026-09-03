# The cycle this program builds on purpose, and the census the
# `ZEO_RT_GCCHECK=1` leg gates against. A cycle alive at exit is not a
# defect: it is a ring the program never broke. What the leg gates is a
# CHANGE to the line below.
# 
# OptionParser's own graph, reached through the required file. See optparse_subset.rb.gccheck.
#@ gccheck: cycle leak: 28 objects (Hash x6, OptionParser::OptionMap x4, Proc x4, cell x4, Array x3, OptionParser::List x2, OptionParser::Switch::OptionalArgument x2, OptionParser x1, OptionParser::Switch::NoArgument x1, OptionParser::Switch::RequiredArgument x1)
# Regression for #1150 bug 2: a `require` on the very first line (byte 0)
# was skipped when another `require` followed it, so optparse.rb was never
# inlined and OptionParser was undefined. Both requires must be processed.
require "optparse"
require "set"
o = OptionParser.new
puts o.class
__END__
OptionParser
