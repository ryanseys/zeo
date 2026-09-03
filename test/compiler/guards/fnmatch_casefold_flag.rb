p File.fnmatch("a*", "A.rb", File::FNM_CASEFOLD)
p File.fnmatch("A*", "a.rb", File::FNM_CASEFOLD)
p File.fnmatch("a*", "A.rb")
__END__
true
true
false
