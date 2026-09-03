# Subclassing a nested error class from outside, raising with a
# qualified path, and rescue-matching through the shared ancestor.

module Errs
  class Base3 < StandardError
  end
end

class Deep < Errs::Base3
end

begin
  raise Deep, "boom"
rescue Errs::Base3 => e
  puts "caught #{e.message}"
end

begin
  raise Errs::Base3, "direct"
rescue StandardError => e
  puts "caught #{e.message}"
end
__END__
caught boom
caught direct
