raise "boom"
__END__
#@ stderr
core/process/uncaught_raise_with_no_rescue_anywhere_exits_with_the_message.rb:1:in '<main>': boom (RuntimeError)
#@ exit 1
