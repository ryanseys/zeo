# $LOAD_PATH (and its `$:` alias) should be the Array of require search
# paths -- zeo leaves it nil instead of populating it.
p $LOAD_PATH.class
p $LOAD_PATH.is_a?(Array)
p $:.class
