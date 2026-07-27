# String#encode only knows a small set of encodings -- "UTF-16" (and
# presumably other non-UTF-8/ASCII encodings) raises ArgumentError
# ("unknown encoding name") instead of transcoding.
p "hello".encode("UTF-16")
