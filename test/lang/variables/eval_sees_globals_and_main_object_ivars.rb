$g = "global"
@iv = 41
puts eval("$g".dup)
puts eval("@iv + 1".dup)
__END__
global
42
