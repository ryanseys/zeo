# The `$$` global answers the running process id, the same value
# `Process.pid` does. Checked by equality and positivity rather than the
# literal number, which necessarily differs between the ruby oracle run and
# the zeo run.
p $$ == Process.pid
p $$.is_a?(Integer) && $$ > 0
