class Plain
end
Plain.new.send(:missing)
__END__
#@ stderr
core/process/an_uncaught_no_method_error_exits_via_the_ordinary_top_level_handler.rb:3:in '<main>': undefined method 'missing' for an instance of Plain (NoMethodError)
#@ exit 1
