# A Hash serializes through the method as well as through JSON.generate and
# JSON.dump.
require "json"
p({a: 1}.to_json)
__END__
"{\"a\":1}"
