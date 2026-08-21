# `String#casecmp` and `Symbol#casecmp` compare ASCII case-insensitively and
# everything else BY BYTES. zeo folds the whole string, so a pair that ruby
# orders as different compares equal.
#
#   "Ä".casecmp("ä")   ruby -1   zeo 0
#   "Д".casecmp("д")   ruby -1   zeo 0
#   :Ä.casecmp(:ä)     ruby -1   zeo 0
#
# This is CRuby's documented split and the reason `casecmp?` exists beside
# it: `casecmp` is the ASCII-only comparison (`rb_str_casecmp`, which walks
# bytes and folds only `A-Z`), while `casecmp?` is the Unicode one
# (`rb_str_casecmp?`, which case-folds both sides first). zeo implements
# both as the Unicode comparison, so `casecmp` answers `casecmp?`'s question.
#
# `casecmp?` itself AGREES with ruby on every pair probed, so this is one
# method, not the family -- and the fix is to stop folding rather than to
# add anything: compare bytes, folding only the ASCII letter range.
#
# Worth keeping apart from `the_case_mapping_methods_take_no_options.rb`:
# that one is about arguments zeo does not accept, this one is about an
# answer zeo gets wrong for a call it does accept -- which is the more
# dangerous of the two, since nothing raises.
#
# Oracle: ASCII folds, the rest compares by bytes.
p "A".casecmp("a")
p "Ä".casecmp("ä")
p "Д".casecmp("д")
p :Ä.casecmp(:ä)
p "Ä".casecmp?("ä")
p "A".casecmp?("a")
p "a".casecmp(1)
p "ä".casecmp("Ä")
p "aÄ".casecmp("aä")
