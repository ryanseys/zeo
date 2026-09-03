begin
  ENV.fetch("SPINEL_NO_SUCH_VAR_XYZ")
rescue KeyError => e
  p e.key
  p e.message
end
p ENV.fetch("SPINEL_NO_SUCH_VAR_XYZ", "dflt")
__END__
"SPINEL_NO_SUCH_VAR_XYZ"
"key not found: \"SPINEL_NO_SUCH_VAR_XYZ\""
"dflt"
