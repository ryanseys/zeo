# JSON.generate's option surface is ignored wholesale: indent/object_nl/
# array_nl/space (so pretty_generate != generate with those opts),
# space_before, ascii_only (must \u-escape), script_safe (must escape /),
# and the generate-side max_nesting/depth NestingError. fast_generate is
# absent.
require "json"
def show
  p yield
rescue Exception => e
  puts "#{e.class}: #{e.message}"
end
show { JSON.generate({ a: [1] }, indent: "  ", object_nl: "\n", array_nl: "\n", space: " ") }
show { JSON.pretty_generate({ a: 1 }) == JSON.generate({ a: 1 }, indent: "  ", object_nl: "\n", array_nl: "\n", space: " ") }
show { JSON.fast_generate({ a: 1 }) }
show { JSON.generate({ a: 1 }, space_before: "_") }
show { JSON.generate({ "k" => "é" }, ascii_only: true) }
show { JSON.generate("</script>", script_safe: true) }
show { JSON.generate([[[1]]], max_nesting: 2) }
__END__
"{\n  \"a\": [\n    1\n  ]\n}"
true
"{\"a\":1}"
"{\"a\"_:1}"
"{\"k\":\"\\u00e9\"}"
"\"<\\/script>\""
JSON::NestingError: nesting of 2 is too deep. Did you try to serialize objects with circular references?
