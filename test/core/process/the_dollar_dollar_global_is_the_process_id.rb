# It equals Process.pid, checked by equality rather than by the number, which
# differs between the two runs.
p $$ == Process.pid
p $$.is_a?(Integer) && $$ > 0
__END__
true
true
