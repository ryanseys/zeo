# `cause:` is the only keyword `raise` reads. Anything else written that way is
# not a keyword at all: ruby collapses the rest into ONE Hash and passes it as
# an ordinary positional argument, so it lands in the MESSAGE slot and reaches
# `TheError.exception(hash)`.
#
# pundit writes `raise NotAuthorizedError, query: query, record: record,
# policy: policy`, and its error class takes a positional options hash. Reading
# those as keywords zeo had to accept made the whole gem a compile error.

class Denied < StandardError
  def initialize(options = {})
    @options = options
    super("denied #{options[:query]}")
  end

  attr_reader :options
end

begin
  raise Denied, query: :show?, record: 7
rescue Denied => e
  p e.message
  p e.options
end

# An error class that takes a real message gets the HASH as its message, which
# is exactly what ruby does -- no keyword is special here either.
begin
  raise ArgumentError, message: "params must be a Hash"
rescue ArgumentError => e
  p e.message
end

prior = RuntimeError.new("prior")

# `cause:` is taken OUT of the collapsed hash, and the rest still collapses.
begin
  raise Denied, query: :edit?, cause: prior
rescue Denied => e
  p e.options
  p e.cause.message
end

# With nothing left after `cause:` comes out, no extra positional is passed at
# all -- `Denied.new` runs on its default.
begin
  raise Denied, cause: prior
rescue Denied => e
  p e.options
  p e.cause.message
end

# `**h` is part of the collapsed hash like any pair.
h = { query: :splatted, record: 1 }

begin
  raise Denied, **h
rescue Denied => e
  p e.options
end

# A `cause:` beside a real message positional leaves the message alone: the
# keyword never reaches the exception at all.
class Simple < StandardError; end

begin
  raise Simple, "explicit", cause: prior
rescue Simple => e
  p e.message
  p e.cause.message
end

# `fail` is the same method under its other name.
begin
  fail Denied, query: :destroy?
rescue Denied => e
  p e.options
end
__END__
"denied show?"
{query: :show?, record: 7}
"{message: \"params must be a Hash\"}"
{query: :edit?}
"prior"
{}
"prior"
{query: :splatted, record: 1}
"explicit"
"prior"
{query: :destroy?}
