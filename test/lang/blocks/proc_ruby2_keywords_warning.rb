pr = proc { |x| x }
pr.ruby2_keywords
p :done
__END__
:done
#@ stderr
lang/blocks/proc_ruby2_keywords_warning.rb:2: warning: Skipping set of ruby2_keywords flag for proc (proc accepts keywords or post arguments or proc does not accept argument splat)
