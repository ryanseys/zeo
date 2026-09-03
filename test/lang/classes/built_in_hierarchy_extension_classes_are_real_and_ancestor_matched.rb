# `KeyError < IndexError` -- exercises the extended built-in hierarchy
# (an addition beyond the original minimal foundation).

begin
  raise KeyError, "missing"
rescue IndexError => e
  puts "caught KeyError via IndexError: #{e.send(:message)}"
end
__END__
caught KeyError via IndexError: missing
