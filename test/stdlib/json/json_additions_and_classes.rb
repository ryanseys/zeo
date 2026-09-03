# `json/add/*` teaches a core class to serialize itself, and `create_additions`
# is what reads it back: a parsed object naming a class under `JSON.create_id`
# becomes that class's `json_create` of itself. INNERMOST FIRST, so an
# addition receives children that are already revived. A name that resolves to
# no constant is an ArgumentError; one whose class has no `json_create` is left
# alone, because `json_class` is an ordinary key until some class claims it.
#
# `parse` leaves the option OFF and `load` turns it ON -- that difference is
# the whole distinction between the two entry points.

require "json"
require "json/add/core"
require "json/add/string"
require "json/add/date"

def t(label)
  r = begin
    yield.inspect
  rescue Exception => e
    "#{e.class}: #{e.message}"
  end
  puts format("%-22s %s", label, r)
end

R = '{"json_class":"Range","a":[1,3,false]}'

t("parse default") { JSON.parse(R) }
t("parse additions") { JSON.parse(R, create_additions: true) }
t("parse off") { JSON.parse(R, create_additions: false) }
t("load default") { JSON.load(R) }
t("load off") { JSON.load(R, nil, create_additions: false) }
t("symbolize+add") { JSON.parse(R, create_additions: true, symbolize_names: true) }

t("range to_json") { (1..3).to_json }
t("exclusive") { JSON.parse((1...3).to_json, create_additions: true) }
t("revived class") { JSON.parse(R, create_additions: true).class }
t("nested") do
  JSON.parse('{"json_class":"Range","a":[{"json_class":"Symbol","s":"a"},' \
             '{"json_class":"Symbol","s":"z"},false]}', create_additions: true)
end
t("in array") { JSON.parse("[#{R}]", create_additions: true) }
t("as value") { JSON.parse(%({"k":#{R}}), create_additions: true) }

t("unknown class") { JSON.parse('{"json_class":"NoSuchThing","a":[]}', create_additions: true) }
t("no json_create") { JSON.parse('{"json_class":"Comparable"}', create_additions: true) }

t("symbol") { JSON.parse(:a.to_json, create_additions: true) }
t("struct") { s = Struct.new("Pair", :x); JSON.parse(s.new(1).to_json, create_additions: true).x }
t("exception") { JSON.parse(RuntimeError.new("boom").to_json, create_additions: true).message }
t("time") { JSON.parse(Time.utc(2000).to_json, create_additions: true).utc.to_s }
t("regexp") { JSON.parse(/ab/i.to_json, create_additions: true).inspect }
t("date") { JSON.parse(Date.new(2001, 2, 3).to_json, create_additions: true).to_s }
t("string") { JSON.parse("abc".to_json, create_additions: true) }

t("create_id") { JSON.create_id }
t("constants") { [defined?(JSON::Fragment), defined?(JSON::GeneratorError), defined?(JSON::GenericObject)] }
t("JSON_LOADED") { JSON::JSON_LOADED }
t("non-finite") { [JSON::NaN.nan?, JSON::Infinity, JSON::MinusInfinity] }
t("fragment") { JSON.generate({ "pre" => JSON::Fragment.new("[1,2]") }) }

# The generator's option bag carries CRuby's whole field list, and is named
# where the C extension names it.
t("state class") { JSON::State.name }
t("state is aliased") { JSON::State.equal?(JSON::Ext::Generator::State) }
t("state to_h") { JSON::State.new.to_h }
t("state predicates") do
  s = JSON::State.new
  [s.allow_nan?, s.ascii_only?, s.script_safe?, s.escape_slash?, s.check_circular?, s.strict?]
end
t("no limit, no circle") { JSON::State.new(max_nesting: 0).check_circular? }
t("state []") { JSON::State.new(indent: "  ")[:indent] }
t("state [] unknown") { JSON::State.new[:nope] }
t("state []= unknown") { s = JSON::State.new; s[:nope] = 1; s[:nope] }
t("sort_keys is a proc") { JSON::State.new(sort_keys: true).sort_keys.class }
t("sort_keys effect") { JSON.generate({ "b" => 1, "a" => 2 }, JSON::State.new(sort_keys: true)) }
t("as_json from symbol") { JSON::State.new(as_json: :itself).as_json.class }
t("from_state nil") { JSON::State.from_state(nil).indent }
t("from_state hash") { JSON::State.from_state({ indent: "\t" }).indent }
t("from_state self") { s = JSON::State.new; JSON::State.from_state(s).equal?(s) }
t("state generate") { JSON::State.new(space: " ").generate({ "a" => 1 }) }
__END__
parse default          {"json_class" => "Range", "a" => [1, 3, false]}
parse additions        1..3
parse off              {"json_class" => "Range", "a" => [1, 3, false]}
load default           1..3
load off               {"json_class" => "Range", "a" => [1, 3, false]}
symbolize+add          ArgumentError: options :symbolize_names and :create_additions cannot be  used in conjunction
range to_json          "{\"json_class\":\"Range\",\"a\":[1,3,false]}"
exclusive              1...3
revived class          Range
nested                 :a..:z
in array               [1..3]
as value               {"k" => 1..3}
unknown class          ArgumentError: can't get const NoSuchThing: uninitialized constant NoSuchThing
no json_create         {"json_class" => "Comparable"}
symbol                 :a
struct                 1
exception              "boom"
time                   "2000-01-01 00:00:00 UTC"
regexp                 "/ab/i"
date                   "2001-02-03"
string                 "abc"
create_id              "json_class"
constants              ["constant", "constant", "constant"]
JSON_LOADED            true
non-finite             [true, Infinity, -Infinity]
fragment               "{\"pre\":[1,2]}"
state class            "JSON::Ext::Generator::State"
state is aliased       true
state to_h             {indent: "", space: "", space_before: "", object_nl: "", array_nl: "", as_json: false, allow_nan: false, ascii_only: false, max_nesting: 100, script_safe: false, strict: false, depth: 0, buffer_initial_length: 1024, sort_keys: false}
state predicates       [false, false, false, false, true, false]
no limit, no circle    false
state []               "  "
state [] unknown       nil
state []= unknown      1
sort_keys is a proc    Proc
sort_keys effect       "{\"a\":2,\"b\":1}"
as_json from symbol    Proc
from_state nil         ""
from_state hash        "\t"
from_state self        true
state generate         "{\"a\": 1}"
