require "yaml"

p({ "x" => 1 }.to_yaml)
p [1, 2].to_yaml
p "s".to_yaml
__END__
"---\nx: 1\n"
"---\n- 1\n- 2\n"
"--- s\n"
