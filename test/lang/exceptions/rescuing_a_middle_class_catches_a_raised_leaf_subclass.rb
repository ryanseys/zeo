class AppError < StandardError
end
class ValidationError < AppError
end

begin
  raise ValidationError, "bad input"
rescue AppError => e
  puts "caught as AppError: #{e.send(:message)}"
end
__END__
caught as AppError: bad input
