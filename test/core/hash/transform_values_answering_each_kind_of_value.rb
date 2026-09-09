# A Float, a String, an Integer and an array, over String and Integer keys.
# (spinel issue #3173)
p({ "a" => 85 }.transform_values { |v| v / 10.0 })
p({ "a" => 1, "b" => 2 }.transform_values { |v| v.to_s })
p({ "a" => 1 }.transform_values { |v| v * 2 })
p({ "a" => 1 }.transform_values { |v| [v, v] })
p({ 1 => 5 }.transform_values { |v| v / 2.0 })
__END__
{"a" => 8.5}
{"a" => "1", "b" => "2"}
{"a" => 2}
{"a" => [1, 1]}
{1 => 2.5}
