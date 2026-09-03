# The two conversion errors carry the encoding pair and the offending input on
# the exception, not only inside the message.

begin
  "\xC2".dup.force_encoding("UTF-8").encode("Shift_JIS")
rescue Encoding::InvalidByteSequenceError => e
  puts "invalid class: #{e.class}"
  puts "invalid source: #{e.source_encoding}"
  puts "invalid source name: #{e.source_encoding_name}"
  puts "invalid destination: #{e.destination_encoding}"
  puts "invalid destination name: #{e.destination_encoding_name}"
  puts "invalid bytes: #{e.error_bytes.inspect}"
  puts "invalid incomplete?: #{e.incomplete_input?}"
  puts "invalid readagain: #{e.readagain_bytes.inspect}"
end

begin
  "é".encode("US-ASCII")
rescue Encoding::UndefinedConversionError => e
  puts "undef class: #{e.class}"
  puts "undef source: #{e.source_encoding}"
  puts "undef source name: #{e.source_encoding_name}"
  puts "undef destination: #{e.destination_encoding}"
  puts "undef destination name: #{e.destination_encoding_name}"
  puts "undef char: #{e.error_char.inspect}"
end

# Reflection files each accessor on the class CRuby owns it on -- and an
# ordinary subclass owns none of them.
puts "invalid own: #{Encoding::InvalidByteSequenceError.instance_methods(false).sort.inspect}"
puts "undef own: #{Encoding::UndefinedConversionError.instance_methods(false).sort.inspect}"
# `respond_to?` is the one row zeo files on Kernel rather than Exception -- an
# accepted owner divergence in the census, subtracted so the rest can be exact.
puts "Exception own: #{(Exception.instance_methods(false).sort - [:respond_to?]).inspect}"
# `#initialize` is Exception's and PRIVATE, so it appears here and in neither
# public listing. (zeo files `method_missing`/`respond_to_missing?` on
# BasicObject rather than restating them here, which reflection does not gate.)
puts "Exception private initialize: #{Exception.private_instance_methods(false).include?(:initialize)}"
puts "KeyError own: #{KeyError.instance_methods(false).sort.inspect}"
puts "NameError own: #{NameError.instance_methods(false).sort.inspect}"

class PlainError < StandardError; end
puts "user subclass own: #{PlainError.instance_methods(false).inspect}"
puts "StandardError own: #{StandardError.instance_methods(false).inspect}"

# `Exception#backtrace_locations` is `#backtrace`'s object form.
begin
  raise "boom"
rescue => e
  puts "locations class: #{e.backtrace_locations.class}"
  puts "locations first: #{e.backtrace_locations.first.class}"
  puts "locations agree: #{e.backtrace_locations.size == e.backtrace.size}"
  puts "location lineno: #{e.backtrace_locations.first.lineno.is_a?(Integer)}"
end
puts "unraised locations: #{RuntimeError.new('x').backtrace_locations.inspect}"

# `NoMatchingPatternKeyError` carries the key and the Hash it was asked of.
err = NoMatchingPatternKeyError.new(matchee: { a: 1 }, key: :b)
puts "pattern key: #{err.key.inspect}"
puts "pattern matchee: #{err.matchee.inspect}"
puts "pattern message: #{err.message}"
bare = NoMatchingPatternKeyError.new
begin
  bare.key
rescue ArgumentError => e
  puts "pattern bare key: #{e.message}"
end
begin
  bare.matchee
rescue ArgumentError => e
  puts "pattern bare matchee: #{e.message}"
end

begin
  nil.no_such_method
rescue NameError => e
  puts "name error locals: #{e.local_variables.class}"
end
__END__
invalid class: Encoding::InvalidByteSequenceError
invalid source: UTF-8
invalid source name: UTF-8
invalid destination: Shift_JIS
invalid destination name: Shift_JIS
invalid bytes: "\xC2"
invalid incomplete?: true
invalid readagain: nil
undef class: Encoding::UndefinedConversionError
undef source: UTF-8
undef source name: UTF-8
undef destination: US-ASCII
undef destination name: US-ASCII
undef char: "é"
invalid own: [:destination_encoding, :destination_encoding_name, :error_bytes, :incomplete_input?, :readagain_bytes, :source_encoding, :source_encoding_name]
undef own: [:destination_encoding, :destination_encoding_name, :error_char, :source_encoding, :source_encoding_name]
Exception own: [:==, :backtrace, :backtrace_locations, :cause, :detailed_message, :exception, :full_message, :inspect, :message, :set_backtrace, :to_s]
Exception private initialize: true
KeyError own: [:key, :receiver]
NameError own: [:local_variables, :name, :receiver]
user subclass own: []
StandardError own: []
locations class: Array
locations first: Thread::Backtrace::Location
locations agree: true
location lineno: true
unraised locations: nil
pattern key: :b
pattern matchee: {a: 1}
pattern message: NoMatchingPatternKeyError
pattern bare key: no key is available
pattern bare matchee: no matchee is available
name error locals: Array
