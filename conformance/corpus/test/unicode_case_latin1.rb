# String case methods handle ASCII + Latin-1 Supplement + Latin Extended-A
# (the common accented Latin ranges), not just ASCII a-z. ß upcases to SS.
p "äöü".upcase
p "ÄÖÜ".downcase
p "aßet".upcase
p "hello WORLD".swapcase
p "ÀÉÎ".downcase
p "àéî".upcase
p "äöü".capitalize
p "ĉĝĥ".upcase
p "ĈĜĤ".downcase
p "HELLO".downcase
p "hello".upcase
p "MixedÄcase".swapcase
