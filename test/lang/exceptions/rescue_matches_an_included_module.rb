module Alertable; end
class AppError < StandardError
  include Alertable
end
begin
  raise AppError, "direct"
rescue Alertable => e
  puts "alertable: #{e.message}"
end
__END__
alertable: direct
