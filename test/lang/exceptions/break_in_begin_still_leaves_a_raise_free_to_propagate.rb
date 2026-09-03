# The loop-crossing settle only translates Break/Next/Redo; a re-raised
# exception still `?`-propagates out to the outer handler (H2).

begin
  i = 0
  while i < 3
    begin
      raise "boom" if i == 1
    rescue => e
      raise "rethrow #{e.message}"
    end
    i += 1
  end
rescue => e
  puts "caught: #{e.message}"
end
__END__
caught: rethrow boom
