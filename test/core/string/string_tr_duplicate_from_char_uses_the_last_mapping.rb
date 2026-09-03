# A char repeated in `from` takes its LAST corresponding `to` char
# (CRuby's rule) -- previously the FIRST mapping wrongly won.

p "a___b".tr("___", ".+-")
p "abcaa".tr("aa", "xy")
p "abcd".tr("abc", "x")
__END__
"a---b"
"ybcyy"
"xxxd"
