# `[[:alpha:]]` matches an accented letter, not only ASCII.
p "café".match?(/\A[[:alpha:]]+\z/)
p "abc".match?(/\A[[:alpha:]]+\z/)
p "é".match?(/[[:alpha:]]/)
__END__
true
true
true
