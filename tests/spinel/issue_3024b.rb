begin; proc { break 7 }.call; rescue LocalJumpError => e001; p e001.exit_value; end
def make001; proc { return 10 }; end
p001 = make001
begin; p001.call; rescue LocalJumpError => e002; p e002.exit_value; end
