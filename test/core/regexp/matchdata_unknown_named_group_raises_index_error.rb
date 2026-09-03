# An unknown named-group key raises IndexError (previously a Rust panic);
# a known name resolves its capture.

m = "2026-06".match(/(?<y>\d+)-(?<mo>\d+)/)
p m[:mo]
begin
  m[:nope]
rescue IndexError => e
  puts "#{e.class}: #{e.message}"
end
__END__
"06"
IndexError: undefined group name reference: nope
