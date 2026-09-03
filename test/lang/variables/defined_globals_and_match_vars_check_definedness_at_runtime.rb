# A user global is "global-variable" only once assigned; a predefined
# special ($~) always is; a match capture only when it participated. An
# array literal is defined only if every element is.

$set_g = 1
p defined?($set_g)
p defined?($never_set_g)
p defined?($~)
p defined?($1)
"hi" =~ /(h)/
p defined?($1)
p defined?($2)
p defined?([1, Array])
p defined?([Nonexist_zzz, Array])
__END__
"global-variable"
nil
"global-variable"
nil
"global-variable"
nil
"expression"
nil
