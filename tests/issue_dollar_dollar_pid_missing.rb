# The `$$` global (process id, an alias for Process.pid) is nil instead of
# the running process's PID -- Process.pid itself works correctly. Checked
# via equality/positivity (not the literal PID) since that number is
# necessarily different between the ruby oracle run and the zeo run.
p $$ == Process.pid
p $$.is_a?(Integer) && $$ > 0
