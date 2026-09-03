# `/(?<a>..)/ =~ str` assigns each named group to a local of that name.
# Only with the literal on the LEFT -- `str =~ /(?<a>.)/` binds nothing,
# which is Ruby's own asymmetry (the parser can only declare the locals
# when it can see the names), not an approximation. Oracle-verified,
# including that a failed match leaves each name nil.

if /(?<first>\w+) (?<last>\w+)/ =~ "John Smith"
  puts first
  puts last
end
p(/(?<n>\d+)/ =~ "abc123")
p n
/(?<z>\d+)/ =~ "none"
p z
__END__
John
Smith
3
"123"
nil
