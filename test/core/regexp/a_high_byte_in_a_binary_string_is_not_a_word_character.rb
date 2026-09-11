# A high byte in a binary string is not a word character and sits at no
# word boundary: `\b` and `[[:word:]]` both miss it, whether it stands alone
# or is the second byte of what would be a UTF-8 sequence in another
# encoding. Character classes in a binary regexp are byte classes.
p "\xB5".b =~ /\b/
p "\xB5".b =~ /[[:word:]]/
p "\xC2\xB5".b =~ /[[:word:]]/
p "\xC2\xB5".b.scan(/\b/).size
__END__
nil
nil
nil
0
