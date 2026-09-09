# The arm that raises and the arm that answers a value coexist; the condition
# is decided at run time.
# (spinel issue #2949)
arr = [nil]
u = arr.first
flag = ARGV.length > 5
label = (flag ? u.details : "~")
puts label
other = (flag ? u.missing : 42)
p other
__END__
~
42
