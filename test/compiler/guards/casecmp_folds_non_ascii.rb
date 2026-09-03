# `String#casecmp` and `Symbol#casecmp` compare ASCII case-insensitively
# and everything else BY BYTES.
#
# That is CRuby's documented split and the reason `casecmp?` exists beside
# it: `casecmp` is `rb_str_casecmp`, which walks bytes and folds only
# `A-Z`, while `casecmp?` is `rb_str_casecmp?`, which case-folds both
# sides first. zeo implemented both as the Unicode comparison, so
# `casecmp` answered `casecmp?`'s question -- `"Ä".casecmp("ä")` was 0
# where ruby says -1, and nothing raised.

p "A".casecmp("a")
p "Ä".casecmp("ä")
p "Д".casecmp("д")
p :Ä.casecmp(:ä)
p "Ä".casecmp?("ä")
p "A".casecmp?("a")
p "a".casecmp(1)
p "ä".casecmp("Ä")
p "aÄ".casecmp("aä")
__END__
0
-1
-1
-1
true
true
nil
1
-1
