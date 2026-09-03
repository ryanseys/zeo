I = Data.define(:v)
x = I.new(1)
p([x])
p({ k: x })
__END__
[#<data I v=1>]
{k: #<data I v=1>}
