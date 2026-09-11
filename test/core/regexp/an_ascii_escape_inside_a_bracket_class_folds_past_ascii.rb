# Under `/i`, `\w` inside a bracket class stays ASCII: neither U+017F
# (long s) nor the Kelvin sign is a word character, in a plain class, an
# intersection, an ASCII-mode class, or a negated `[^\W]`. Only `[a-z\w]`
# matches the long s, through the range rather than the escape. zeo folds the
# class escape past ASCII, so several of these match where ruby misses.
p "ſ" =~ /[\w]/i
p "K" =~ /[\w]/i
p "ſ" =~ /[\w&&[a-z]]/i
p "K" =~ /(?a)[[:word:]]/i
p "ſ" =~ /[a-z\w]/i
p "ſ" =~ /[^\W]/i
__END__
nil
nil
nil
nil
0
nil
