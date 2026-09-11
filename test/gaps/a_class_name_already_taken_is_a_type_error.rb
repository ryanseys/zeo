# Ruby raises before anything is minted, and the TypeError's message has a
# second line naming where TOP was defined. zeo's message stops after the
# first line.

TOP = 1
src = "class TOP; end"
begin
  eval(src)
rescue TypeError => e
  puts e.message
end
__END__
TOP is not a class
gaps/a_class_name_already_taken_is_a_type_error.rb:5: previous definition of TOP was here
