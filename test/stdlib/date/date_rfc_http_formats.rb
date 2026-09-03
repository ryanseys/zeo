require "date"

d = Date.new(2024, 2, 29)
p d.rfc3339
p d.httpdate
p d.xmlschema
p d.jisx0301
__END__
"2024-02-29T00:00:00+00:00"
"Thu, 29 Feb 2024 00:00:00 GMT"
"2024-02-29"
"R06.02.29"
