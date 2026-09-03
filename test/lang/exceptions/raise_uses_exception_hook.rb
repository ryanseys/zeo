class CustomE < StandardError
  def self.exception(msg = nil) = new("custom: #{msg}")
end

begin
  raise CustomE, "x"
rescue => e
  p e.message
end
__END__
"custom: x"
