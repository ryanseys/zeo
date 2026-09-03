# Psych resolves plain scalars by YAML 1.1 rules: yes/no/on/off (any
# case) are booleans, Null/NULL are nil, 017 is OCTAL 15, 1:02:03 is the
# sexagesimal 3723, and "1.0e3"/"1E2" stay STRINGS (the 1.1 float form
# needs a digit after the point). zeo's yaml-rust2 resolver applies 1.2
# rules for all of these -- the Norway problem in both directions.
# (Found by the 2026-08-24 probe sweep.)
require "yaml"
p YAML.safe_load("[y, Y, yes, Yes, YES, n, no, No, NO, on, On, off, true, false]")
p YAML.safe_load("[~, null, Null, NULL, '']")
p YAML.safe_load("[017, 0x1A, 1_000, +5]")
p YAML.safe_load("t: 1:02:03")
p YAML.safe_load("[1.0e3, .5, 5., 1E2]")
p YAML.safe_load("no: 1\nyes: 2").keys
__END__
["y", "Y", true, true, true, "n", false, false, false, true, true, false, true, false]
[nil, nil, nil, nil, ""]
[15, 26, 1000, 5]
{"t" => 3723}
["1.0e3", 0.5, 5.0, "1E2"]
[false, true]
