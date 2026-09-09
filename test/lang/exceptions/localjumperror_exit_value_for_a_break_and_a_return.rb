# A proc that breaks and a proc that returns each raise LocalJumpError, and exit_value carries the argument.
# (spinel issue #3024b)
begin; proc { break 7 }.call; rescue LocalJumpError => e001; p e001.exit_value; end
def make001; proc { return 10 }; end
p001 = make001
begin; p001.call; rescue LocalJumpError => e002; p e002.exit_value; end
__END__
7
10
