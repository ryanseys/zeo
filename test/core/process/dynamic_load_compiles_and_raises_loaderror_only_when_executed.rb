# Guarded dynamic load: guard false -> never executes -> no error.
path = nil
load path if path
puts "guard_ok"

# A dynamic load that DOES execute raises a rescuable LoadError.
missing = "/no/such/file.rb"
begin
  load missing
rescue LoadError => e
  puts e.message
end

# A dynamic require behaves the same, and the program continues.
name = "definitely_missing_lib_" + "xyz"
begin
  require name
rescue LoadError => e
  puts e.message
end
puts "done"
__END__
guard_ok
cannot load such file -- /no/such/file.rb
cannot load such file -- definitely_missing_lib_xyz
done
