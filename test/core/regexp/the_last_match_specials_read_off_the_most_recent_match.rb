if "hello world" =~ /(\w+)\s(\w+)/
  p $1
  p $2
  p $3
  p $&
  p $`
  p $'
  p $~[0]
  p $~.class
end
__END__
"hello"
"world"
nil
"hello world"
""
""
"hello world"
MatchData
