# ENV.fetch raises KeyError whose key and message name the variable, and takes a default instead.
begin
  ENV.fetch("PROBE_NO_SUCH_VAR_XYZ")
rescue KeyError => e
  p e.key
  p e.message
end
p ENV.fetch("PROBE_NO_SUCH_VAR_XYZ", "dflt")
__END__
"PROBE_NO_SUCH_VAR_XYZ"
"key not found: \"PROBE_NO_SUCH_VAR_XYZ\""
"dflt"
