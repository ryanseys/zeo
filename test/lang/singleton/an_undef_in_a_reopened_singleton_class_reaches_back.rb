# `class << self; undef away; end` written in a LATER reopening retires the
# class method from that point on. zeo retires it for the whole program, so
# the call written before the reopening raises NoMethodError instead of
# answering `:here`.
#
# Pre-existing, and unrelated to class-method call-site caching: the same call
# raises identically with every `Target.name` site uncached. The `undef` is
# recorded by `class_method_undefined`, and the probe that reads it in
# `send_value_in_reason` is gated only on `is_live()` -- a whole-program flag
# with no notion of when the undef ran. Same shape as
# `a_define_singleton_method_on_a_class_does_not_reach_back.rb`.
class Gone
  def self.away = :here
end
p Gone.away
class Gone
  class << self
    undef away
  end
end
begin
  Gone.away
rescue NoMethodError
  puts "away is gone"
end
__END__
:here
away is gone
