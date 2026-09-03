# Ruby isolates the Proc and rebinds its `self` to the RACTOR, so `@n` is
# an ivar of a shareable object -- `Ractor::IsolationError` in the ractor,
# surfacing as `Ractor::RemoteError` at `#value`. zeo used to refuse to
# compile it.

Thread.report_on_exception = false
class Holder
  def initialize = @n = 7
  def go
    Ractor.new { @n }.value
  rescue Ractor::RemoteError => e
    puts "cause: #{e.cause.class}: #{e.cause.send(:message)}"
  end
end
Holder.new.go
__END__
cause: Ractor::IsolationError: can not access instance variables of shareable objects from non-main Ractors
#@ stderr
core/thread/a_ractor_block_reading_an_enclosing_ivar_raises_inside_the_ractor.rb:10: warning: Ractor API is experimental and may change in future versions of Ruby.
