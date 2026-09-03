# A handler's `exit` overrides the process status (checked by the e2e
# suite); the REMAINING handlers still run -- the observable rule here.
at_exit { puts "h1" }
at_exit { puts "h2"; exit 42 }
at_exit { puts "h3" }
puts "body"
__END__
body
h3
h2
h1
#@ exit 42
