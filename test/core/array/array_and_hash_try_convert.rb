p Array.try_convert([1, 2])
p Array.try_convert("no")
p Hash.try_convert({ a: 1 })
p Hash.try_convert(5)
__END__
[1, 2]
nil
{a: 1}
nil
