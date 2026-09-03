# They are RESET, not left holding the previous match -- the property
# that makes `if s =~ re then $1 end` safe to reuse in a loop.

"ab" =~ /(a)/
p $1
"zzz" =~ /(\d+)/
p $1
p $&
p $~
__END__
"a"
nil
nil
nil
