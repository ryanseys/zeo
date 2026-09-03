# Exercises the same real-`Proc`/capture machinery every other escaping
# block already uses (`emit_proc_value`) -- not a bespoke code path.

count = 0
"one two three".gsub(/\w+/) { |w| count += 1; w }
puts count
__END__
3
