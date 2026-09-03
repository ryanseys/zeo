class Attempt
  def initialize
    @attempts = 0
  end
  def run
    begin
      @attempts += 1
      raise "fail" if @attempts < 3
      puts "succeeded after #{@attempts} attempts"
    rescue
      retry if @attempts < 3
    ensure
      puts "ensure ran, attempts=#{@attempts}"
    end
  end
end
Attempt.new.run
__END__
succeeded after 3 attempts
ensure ran, attempts=3
