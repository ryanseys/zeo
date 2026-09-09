# `$LOAD_PATH` and its `$:` alias are the Array of require search paths.
p $LOAD_PATH.class
p $LOAD_PATH.is_a?(Array)
p $:.class
__END__
Array
true
Array
