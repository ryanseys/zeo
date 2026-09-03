p "a\#{x}".inspect
p "a#b".dump
p "é".dump
p "é".inspect
p "a#$g".dump
p "a#@i".inspect
p "plain".dump
p "tab\there".dump
p "\u{1F600}".dump
__END__
"\"a\\\#{x}\""
"\"a#b\""
"\"\\u00E9\""
"\"é\""
"\"a\""
"\"a\""
"\"plain\""
"\"tab\\there\""
"\"\\u{1F600}\""
