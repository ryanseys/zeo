require "json"
puts JSON.generate({"a" => 1, "b" => [2, 3.5, nil, true]})
puts JSON.parse('{"x":[1,2,3]}').inspect
puts JSON.pretty_generate({"k" => [1, 2]})
__END__
{"a":1,"b":[2,3.5,null,true]}
{"x" => [1, 2, 3]}
{
  "k": [
    1,
    2
  ]
}
