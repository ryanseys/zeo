# An exception inside an at_exit handler is reported immediately, and the
# remaining handlers still run (CRuby's rule; the status becomes 1).
at_exit { puts "h-after" }
at_exit { raise "handler boom" }
at_exit { puts "h-before" }
puts "body"
__END__
body
h-before
h-after
#@ stderr
core/process/at_exit_handler_raise_reports.rb:4:in 'block in <main>': handler boom (RuntimeError)
#@ exit 1
