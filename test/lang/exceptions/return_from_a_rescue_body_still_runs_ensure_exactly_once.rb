class ReturnTester
  def m
    begin
      raise "x"
    rescue
      return "returned"
    ensure
      puts "ensure ran before return"
    end
  end
end
puts ReturnTester.new.m
__END__
ensure ran before return
returned
