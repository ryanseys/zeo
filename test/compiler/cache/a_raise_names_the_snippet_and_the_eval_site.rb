src = "raise 'boom'"
begin
  eval(src)
rescue => e
  puts e.backtrace.first(2)
end
__END__
(eval at compiler/cache/a_raise_names_the_snippet_and_the_eval_site.rb:3):1:in '<main>'
compiler/cache/a_raise_names_the_snippet_and_the_eval_site.rb:3:in 'Kernel#eval'
