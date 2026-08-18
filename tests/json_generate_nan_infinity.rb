require "json"

begin
  JSON.generate({ "a" => Float::NAN })
rescue JSON::GeneratorError => e
  p e.class
end
begin
  JSON.generate({ "a" => Float::INFINITY })
rescue JSON::GeneratorError => e
  p e.class
end
p JSON.generate({ "a" => Float::NAN }, allow_nan: true)
